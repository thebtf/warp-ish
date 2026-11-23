#requires -Version 7.0
param(
    [Parameter(Mandatory)][string]$WrapperSource,
    [string]$ResourcePreparerSource = (Join-Path (Get-Location).ProviderPath 'script/windows/prepare_bundled_resources.ps1')
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repo = (Get-Location).ProviderPath
$savedPath = $env:PATH
$savedMode = $env:PACKAGE_FIXTURE_MODE
$savedToolchain = $env:RUSTUP_TOOLCHAIN
$realGit = (Get-Command git -CommandType Application | Select-Object -First 1).Source
$gitCommonDir = & $realGit -C $repo rev-parse --path-format=absolute --git-common-dir
if ($LASTEXITCODE -ne 0 -or -not [IO.Path]::IsPathFullyQualified($gitCommonDir)) { throw 'Cannot resolve fixture scratch root' }
$mainCheckout = [IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($gitCommonDir))
$fixture = Join-Path $mainCheckout ('.agent\tmp\package fixture [' + [guid]::NewGuid().ToString('N') + ']')
function Write-Fixture([string]$Relative, [string]$Text) {
    $path = Join-Path $fixture $Relative
    [void][IO.Directory]::CreateDirectory((Split-Path $path -Parent))
    [IO.File]::WriteAllText($path, $Text)
}
function Assert-That([bool]$Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function Run-Package([string]$Version, [bool]$Success, [string]$Checkout = $fixture, [string]$ExpectedMessage) {
    $output = & (Join-Path $PSHOME 'pwsh.exe') -NoProfile -File (Join-Path $Checkout 'script\warp-ish\package-windows.ps1') -Version $Version 2>&1
    $code = $LASTEXITCODE
    Assert-That (($code -eq 0) -eq $Success) "Unexpected exit $code for $Version : $output"
    if ($ExpectedMessage) {
        Assert-That (($output -join "`n") -match [regex]::Escape($ExpectedMessage)) "Expected '$ExpectedMessage' for $Version : $output"
    }
}
try {
    Write-Fixture '.gitignore' ".agent/`ntarget/`n"
    Write-Fixture 'Cargo.lock' '# fixture lock'
    Write-Fixture 'rust-toolchain.toml' "[toolchain]`nchannel = `"1.92.0`"`n"
    Write-Fixture 'script/install_cargo_release_deps' "CARGO_ABOUT_VERSION=`"0.8.4`"`r`n"
    Write-Fixture 'bin/cargo-about.ps1' "'cargo-about 0.8.4'; exit 0"
    Write-Fixture 'bin/rustc.ps1' @'
if ($args[0] -ne '+1.92.0') { exit 20 }
if ($env:PACKAGE_FIXTURE_MODE -eq 'toolchain-mismatch') { 'rustc 9.9.9' } else { 'rustc 1.92.0' }
'host: x86_64-pc-windows-msvc'
exit 0
'@
    Write-Fixture 'bin/git.ps1' @'
if ($args[2] -eq 'status') {
    if ((Get-Content -LiteralPath (Join-Path $args[1] 'Cargo.lock') -Raw).Trim() -ne '# fixture lock') { ' M Cargo.lock' }
    if ('--untracked-files=no' -notin $args) {
        foreach ($runtime in @('.agent/continuity', '.agent/session-checkpoints', '.agent/tmp', '.serena')) {
            if (":(top,exclude,glob)$runtime/**" -notin $args) { "?? $runtime/fixture-state" }
        }
        if (Test-Path -LiteralPath (Join-Path $args[1] 'app/src/untracked.rs')) { '?? app/src/untracked.rs' }
    }
} elseif ($args[2] -eq 'rev-parse') {
    if ($args[3] -eq 'HEAD') { 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' }
    elseif ($args[3] -eq 'HEAD^{tree}') { 'cccccccccccccccccccccccccccccccccccccccc' }
    elseif ($args[3] -eq '--path-format=absolute') {
        $checkout = [IO.DirectoryInfo]::new($args[1])
        $managed = $checkout.Name -eq 'WARP-FIXTURE'
        $root = if ($managed) { $checkout.Parent.Parent.Parent.FullName } else { $checkout.FullName }
        switch ($args[4]) {
            '--show-toplevel' { $checkout.FullName }
            '--git-common-dir' {
                if ($managed -and $env:PACKAGE_FIXTURE_MODE -eq 'unrelated-common') {
                    Join-Path $root '.agent/tmp/pathspec-probe/.git'
                } else { Join-Path $root '.git' }
            }
            '--git-dir' {
                if ($managed) { Join-Path $root '.git/worktrees/WARP-FIXTURE' }
                else { Join-Path $root '.git' }
            }
            default { exit 23 }
        }
    }
    else { exit 23 }
} elseif ($args[2] -ne 'merge-base') { exit 24 }
exit 0
'@
    Write-Fixture 'bin/cargo.ps1' @'
if ($args[0] -eq '+1.92.0') { $args = $args[1..($args.Count - 1)] }
elseif ($args[0] -ne 'about') { exit 21 }
if ($args[0] -eq '--version') {
    if ($env:PACKAGE_FIXTURE_MODE -eq 'cargo-mismatch') { 'cargo 9.9.9' } else { 'cargo 1.92.0' }
    exit 0
}
if ($args[0] -eq 'about') {
    if ($env:RUSTUP_TOOLCHAIN -ne '1.92.0') { exit 22 }
    if ('--workspace' -in $args -or '--locked' -notin $args) { exit 31 }
    $manifest = [array]::IndexOf($args, '--manifest-path')
    $features = [array]::IndexOf($args, '--features')
    $target = [array]::IndexOf($args, '--target')
    if ($manifest -lt 0 -or $features -lt 0 -or $target -lt 0 -or
        $args[$manifest + 1] -ne (Join-Path (Get-Location).ProviderPath 'app\Cargo.toml') -or
        $args[$features + 1] -ne 'release_bundle gui' -or
        $args[$target + 1] -ne 'x86_64-pc-windows-msvc') { exit 32 }
    $index = [array]::IndexOf($args, '-o')
    'Fixture dependency attribution' | Set-Content -LiteralPath $args[$index + 1]
    exit 0
}
if ($args[0] -ne 'build' -or '-j' -notin $args -or '--locked' -notin $args) { exit 19 }
if ($env:PACKAGE_FIXTURE_MODE -in @('managed', 'concurrent')) {
    $checkout = [IO.DirectoryInfo]::new((Get-Location).ProviderPath)
    $expectedBuild = Join-Path $checkout.Parent.Parent.Parent.FullName '.agent/build'
    if ($checkout.Name -ne 'WARP-FIXTURE' -or
        (Split-Path -Parent $env:CARGO_TARGET_DIR) -ne $expectedBuild -or
        -not (Test-Path -LiteralPath (Join-Path $checkout.FullName 'target/rlto') -PathType Container)) { exit 25 }
    'fixture side output' | Set-Content -LiteralPath (Join-Path $checkout.FullName 'target/rlto/fixture.txt')
    if ($env:PACKAGE_FIXTURE_MODE -eq 'concurrent') {
        $scratch = Join-Path $checkout.Parent.Parent.Parent.FullName '.agent/tmp'
        [IO.File]::WriteAllText((Join-Path $scratch 'package-ready'), 'cargo is running')
        $deadline = [DateTime]::UtcNow.AddSeconds(20)
        while (-not [IO.File]::Exists((Join-Path $scratch 'package-release'))) {
            if ([DateTime]::UtcNow -ge $deadline) { exit 26 }
            Start-Sleep -Milliseconds 50
        }
    }
}
$output = Join-Path $env:CARGO_TARGET_DIR 'x86_64-pc-windows-msvc/rlto'
[void][IO.Directory]::CreateDirectory($output)
$binary = Join-Path $output 'warp-oss.exe'
if ($env:PACKAGE_FIXTURE_MODE -eq 'hardlink') {
    $deps = Join-Path $output 'deps'
    [void][IO.Directory]::CreateDirectory($deps)
    $artifact = Join-Path $deps 'warp_oss-fixture.exe'
    'fixture executable' | Set-Content -LiteralPath $artifact
    [void](New-Item -ItemType HardLink -Path $binary -Target $artifact)
    if ((Get-Item -LiteralPath $binary).LinkType -ne 'HardLink') { exit 29 }
} else {
    'fixture executable' | Set-Content -LiteralPath $binary
}
if ($env:PACKAGE_FIXTURE_MODE -eq 'fail') { exit 7 }
if ($env:PACKAGE_FIXTURE_MODE -eq 'drift') { Add-Content -LiteralPath Cargo.lock -Value '# changed during build' }
exit 0
'@
    Write-Fixture 'script/warp-ish/package-windows.ps1' (Get-Content -LiteralPath $WrapperSource -Raw)
    $prepare = Get-Content -LiteralPath $ResourcePreparerSource -Raw
    # Keep real resource/licensing/allowlist behavior; replace only native schema process.
    $mock = @'
function Start-Process {
    param($FilePath, $ArgumentList, [switch]$NoNewWindow, [switch]$PassThru, [switch]$Wait)
    if ($ArgumentList[0] -ne 'dump-settings-schema' -or
        (Get-Content -LiteralPath $FilePath -Raw).Trim() -ne 'fixture executable') { throw 'Wrong schema executable' }
    $schema = @{ type = 'object'; generated_by_sha256 = (Get-FileHash -LiteralPath $FilePath).Hash }
    $schema | ConvertTo-Json | Set-Content -LiteralPath $ArgumentList[1].Trim('"')
    return @{ ExitCode = 0 }
}
'@
    $prepare = $prepare.Replace("`$ErrorActionPreference = 'Stop'", "`$ErrorActionPreference = 'Stop'`n$mock")
    Write-Fixture 'script/windows/prepare_bundled_resources.ps1' $prepare
    foreach ($asset in @('conpty.dll', 'OpenConsole.exe', 'vcruntime140.dll', 'vcruntime140_1.dll', 'msvcp140.dll', 'dxcompiler.dll', 'dxil.dll')) {
        Write-Fixture "app/assets/windows/x64/$asset" "fixture $asset"
    }
    Write-Fixture 'app/assets/bundled/bootstrap/pwsh.ps1' '# fixture bootstrap'
    Write-Fixture 'app/channels/oss/icon/no-padding/icon.ico' 'fixture icon'
    $licensePaths = [regex]::Matches($prepare, "Path = '([^']+)'") | ForEach-Object { $_.Groups[1].Value }
    foreach ($path in $licensePaths) { Write-Fixture $path 'Fixture additional license' }
    [void][IO.Directory]::CreateDirectory((Join-Path $fixture 'resources'))
    foreach ($runtime in @('.agent/continuity', '.agent/session-checkpoints', '.agent/tmp', '.serena')) {
        Write-Fixture "$runtime/fixture-state" 'valuable runtime state'
    }
    # Test the actual Git pathspec independently of the process stubs, without commits.
    & $realGit -C $fixture init --quiet
    if ($LASTEXITCODE -ne 0) { throw 'Cannot initialize package fixture metadata' }
    $pathspecRepo = Join-Path $fixture '.agent/tmp/pathspec-probe'
    [void][IO.Directory]::CreateDirectory($pathspecRepo)
    & $realGit -C $pathspecRepo init --quiet
    if ($LASTEXITCODE -ne 0) { throw 'Cannot initialize pathspec fixture' }
    foreach ($relative in @('.agent/continuity/state', '.agent/session-checkpoints/state', '.agent/tmp/state', '.serena/state', 'app/src/untracked.rs')) {
        $path = Join-Path $pathspecRepo $relative
        [void][IO.Directory]::CreateDirectory((Split-Path $path -Parent))
        [IO.File]::WriteAllText($path, 'fixture')
    }
    $probeArguments = @('-C', $pathspecRepo, 'status', '--porcelain', '--untracked-files=all', '--', '.',
        ':(top,exclude,glob).agent/continuity/**', ':(top,exclude,glob).agent/session-checkpoints/**',
        ':(top,exclude,glob).agent/tmp/**', ':(top,exclude,glob).serena/**')
    $unknown = @(& $realGit @probeArguments)
    Assert-That ($LASTEXITCODE -eq 0 -and $unknown.Count -eq 1 -and $unknown[0] -eq '?? app/src/untracked.rs') 'Actual Git pathspec hid product input or included runtime state'
    $upstream = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'
    Write-Fixture 'script/warp-ish/terminal-policy.json' (@{ observed_upstream_sha = $upstream; source_repository = 'fixture' } | ConvertTo-Json)
    $env:PATH = (Join-Path $fixture 'bin') + [IO.Path]::PathSeparator + $savedPath
    $env:PACKAGE_FIXTURE_MODE = 'hardlink'
    $env:RUSTUP_TOOLCHAIN = 'fixture-hostile-override'
    Run-Package '0.1.0-fixture' $true
    Assert-That ($env:RUSTUP_TOOLCHAIN -eq 'fixture-hostile-override') 'Caller toolchain override was not restored'
    $package = Join-Path $fixture '.agent/build/packages/0.1.0-fixture'
    Assert-That ((Get-Item -LiteralPath (Join-Path $package 'app/warp-ish.exe')).LinkType -ne 'HardLink') 'Package executable is not an independent file'
    $record = Get-Content -LiteralPath (Join-Path $package 'package.json') -Raw | ConvertFrom-Json
    Assert-That ($record.archive_sha256 -eq (Get-FileHash -LiteralPath (Join-Path $package $record.archive)).Hash.ToLowerInvariant()) 'Archive binding failed'
    Assert-That ($record.manifest_sha256 -eq (Get-FileHash -LiteralPath (Join-Path $package 'manifest.json')).Hash.ToLowerInvariant()) 'Manifest binding failed'
    $manifest = Get-Content -LiteralPath (Join-Path $package 'manifest.json') -Raw | ConvertFrom-Json
    Assert-That ($manifest.files.Count -eq 13) 'Incorrect complete installer payload count'
    $schema = Get-Content -LiteralPath (Join-Path $package 'app/resources/settings_schema.json') -Raw | ConvertFrom-Json
    Assert-That ($schema.generated_by_sha256 -eq (Get-FileHash -LiteralPath (Join-Path $package 'app/warp-ish.exe')).Hash) 'Schema did not use staged executable'
    Run-Package '0.1.0-fixture' $false
    Assert-That ($record.archive_sha256 -eq (Get-FileHash -LiteralPath (Join-Path $package $record.archive)).Hash.ToLowerInvariant()) 'Existing package overwritten'
    Write-Fixture 'app/src/untracked.rs' '// unknown product source'
    Run-Package '0.1.0-untracked-source' $false
    Assert-That (-not (Test-Path -LiteralPath (Join-Path $fixture '.agent/build/packages/0.1.0-untracked-source'))) 'Untracked source promoted'
    [IO.File]::Delete((Join-Path $fixture 'app/src/untracked.rs'))
    foreach ($mode in @('toolchain-mismatch', 'cargo-mismatch')) {
        $env:PACKAGE_FIXTURE_MODE = $mode
        Run-Package "0.1.0-$mode" $false
        Assert-That (-not (Test-Path -LiteralPath (Join-Path $fixture ".agent/build/packages/0.1.0-$mode"))) 'Toolchain mismatch promoted'
    }
    foreach ($mode in @('fail', 'drift')) {
        $env:PACKAGE_FIXTURE_MODE = $mode
        Run-Package "0.1.0-$mode" $false
        Assert-That (-not (Test-Path -LiteralPath (Join-Path $fixture ".agent/build/packages/0.1.0-$mode"))) 'Failed package promoted'
        Assert-That (-not (Test-Path -LiteralPath (Join-Path $fixture 'target/rlto'))) 'Side outputs not cleaned'
        $targets = @(Get-ChildItem -LiteralPath (Join-Path $fixture '.agent/build') -Directory | Where-Object Name -like 'package-*')
        Assert-That ($targets.Count -eq 0) 'Dedicated target not cleaned'
        $incomplete = @(Get-ChildItem -LiteralPath (Join-Path $fixture '.agent/build/packages') -Directory -Force | Where-Object Name -like '*.incomplete')
        Assert-That ($incomplete.Count -eq 0) 'Failed staging not cleaned'
    }
    $states = @(Get-ChildItem -LiteralPath (Join-Path $fixture '.agent/runs') -Filter state.json -Recurse | ForEach-Object { Get-Content -LiteralPath $_.FullName -Raw | ConvertFrom-Json })
    Assert-That ($states.Count -eq 3 -and @($states | Where-Object { -not $_.build_targets_cleaned }).Count -eq 0) 'Run evidence or cleanup missing'
    foreach ($state in @($states | Where-Object status -eq 'failed')) {
        Assert-That ($state.failed_stage_removed -and $null -ne $state.failed_stage_files) 'Failed stage cleanup or evidence missing'
    }

    # Mirror the registered-worktree path contract without touching shared Git metadata.
    Write-Fixture 'Cargo.lock' '# fixture lock'
    $managed = Join-Path $fixture '.work/wt/WARP-FIXTURE'
    [void][IO.Directory]::CreateDirectory($managed)
    $managedGitDir = Join-Path $fixture '.git/worktrees/WARP-FIXTURE'
    [void][IO.Directory]::CreateDirectory($managedGitDir)
    $managedGitFile = Join-Path $managed '.git'
    [IO.File]::WriteAllText($managedGitFile, "gitdir: $managedGitDir")
    $backlink = Join-Path $managedGitDir 'gitdir'
    [IO.File]::WriteAllText($backlink, $managedGitFile)
    foreach ($entry in @('app', 'crates', 'resources', 'script', 'Cargo.lock', 'rust-toolchain.toml', '.gitignore')) {
        Copy-Item -LiteralPath (Join-Path $fixture $entry) -Destination $managed -Recurse
    }
    Write-Fixture 'target/rlto/sentinel.txt' 'main checkout build state'
    $env:PACKAGE_FIXTURE_MODE = 'managed'
    Run-Package '0.1.0-managed' $true $managed
    $managedPackage = Join-Path $fixture '.agent/build/packages/0.1.0-managed'
    $managedRecord = Get-Content -LiteralPath (Join-Path $managedPackage 'package.json') -Raw | ConvertFrom-Json
    $managedArchiveHash = (Get-FileHash -LiteralPath (Join-Path $managedPackage $managedRecord.archive)).Hash.ToLowerInvariant()
    Assert-That ($managedArchiveHash -eq $managedRecord.archive_sha256) 'Managed package archive binding failed'
    Assert-That (-not (Test-Path -LiteralPath (Join-Path $managed '.agent'))) 'Operational state was written into the managed checkout'
    Assert-That (-not (Test-Path -LiteralPath (Join-Path $managed 'target/rlto'))) 'Managed side output was not cleaned'
    Assert-That (Test-Path -LiteralPath (Join-Path $fixture 'target/rlto/sentinel.txt')) 'Managed cleanup touched the main checkout side output'
    $managedRuns = @(Get-ChildItem -LiteralPath (Join-Path $fixture '.agent/runs') -Directory -Filter 'package-0.1.0-managed-*')
    Assert-That ($managedRuns.Count -eq 1) 'Managed package evidence missing from the shared operations root'
    $managedState = Get-Content -LiteralPath (Join-Path $managedRuns[0].FullName 'state.json') -Raw | ConvertFrom-Json
    Assert-That ($managedState.build_targets_cleaned -and $managedState.package -eq $managedPackage) 'Managed package cleanup or run binding failed'
    Run-Package '0.1.0-managed' $false $managed 'Refusing to overwrite existing path'
    Assert-That ((Get-FileHash -LiteralPath (Join-Path $managedPackage $managedRecord.archive)).Hash.ToLowerInvariant() -eq $managedArchiveHash) 'Managed package overwritten'

    $env:PACKAGE_FIXTURE_MODE = 'unrelated-common'
    Run-Package '0.1.0-unrelated' $false $managed 'Checkout is not a managed worktree'
    $env:PACKAGE_FIXTURE_MODE = 'managed'
    [IO.File]::WriteAllText($backlink, (Join-Path $fixture '.git'))
    Run-Package '0.1.0-wrong-backlink' $false $managed 'Git worktree backlink does not identify this checkout'
    [IO.File]::WriteAllText($backlink, [IO.Path]::GetRelativePath($managedGitDir, $managedGitFile))
    foreach ($name in @('GIT_DIR', 'GIT_COMMON_DIR', 'GIT_WORK_TREE', 'GIT_INDEX_FILE')) {
        $previous = [Environment]::GetEnvironmentVariable($name, 'Process')
        try {
            [Environment]::SetEnvironmentVariable($name, $managedGitDir, 'Process')
            Run-Package '0.1.0-git-override' $false $managed "Unset $name"
        } finally {
            if ($null -eq $previous) { Remove-Item -LiteralPath "Env:$name" }
            else { [Environment]::SetEnvironmentVariable($name, $previous, 'Process') }
        }
    }

    $env:PACKAGE_FIXTURE_MODE = 'concurrent'
    $scratch = Join-Path $fixture '.agent/tmp'
    $firstArguments = @('-NoProfile', '-File', ('"' + (Join-Path $managed 'script/warp-ish/package-windows.ps1') + '"'), '-Version', '0.1.0-concurrent-first')
    $first = Start-Process -FilePath (Join-Path $PSHOME 'pwsh.exe') -ArgumentList $firstArguments -WindowStyle Hidden -PassThru
    try {
        $deadline = [DateTime]::UtcNow.AddSeconds(15)
        while (-not [IO.File]::Exists((Join-Path $scratch 'package-ready'))) {
            if ($first.HasExited -or [DateTime]::UtcNow -ge $deadline) {
                throw 'First package never reached the build'
            }
            Start-Sleep -Milliseconds 50
        }
        Run-Package '0.1.0-concurrent-second' $false $managed 'Another packaging process already owns this checkout'
        $firstSideOutput = Join-Path $managed 'target/rlto/fixture.txt'
        Assert-That ((Get-Content -LiteralPath $firstSideOutput -Raw).Trim() -eq 'fixture side output') 'Second process touched the first process side output'
        Assert-That (-not (Test-Path -LiteralPath (Join-Path $fixture '.agent/build/packages/0.1.0-concurrent-second'))) 'Second process promoted a package'
    } finally {
        [IO.File]::WriteAllText((Join-Path $scratch 'package-release'), 'finish first build')
        if (-not $first.WaitForExit(20000)) { throw 'First package did not finish after release' }
        $firstCode = $first.ExitCode
        $first.Dispose()
    }
    Assert-That ($firstCode -eq 0) "First concurrent package failed with exit code $firstCode"
    Assert-That (Test-Path -LiteralPath (Join-Path $fixture '.agent/build/packages/0.1.0-concurrent-first/package.json')) 'First process did not promote its package'
    Assert-That (-not (Test-Path -LiteralPath (Join-Path $managed 'target/rlto'))) 'First process did not clean its side output'
    $env:PACKAGE_FIXTURE_MODE = 'managed'
    Run-Package '0.1.0-concurrent-second' $true $managed

    $junction = Join-Path $managed 'target/rlto'
    [void](New-Item -ItemType Junction -Path $junction -Target (Join-Path $fixture 'target/rlto'))
    Run-Package '0.1.0-reparse' $false $managed 'Reparse point in package path'
    Assert-That (Test-Path -LiteralPath (Join-Path $fixture 'target/rlto/sentinel.txt')) 'Reparse refusal touched its destination'

    'PASS: payload and ZIP/schema/source binding, pinned toolchain, overwrite and cleanup protection, Git override and backlink refusal, same-checkout process exclusion, managed side-output isolation and reparse refusal.'
} finally {
    $env:PATH = $savedPath
    $env:PACKAGE_FIXTURE_MODE = $savedMode
    $env:RUSTUP_TOOLCHAIN = $savedToolchain
    # Fixture retained for inspection; all writes are under the repository ignored scratch.
    Write-Output "Fixture: $fixture"
}
