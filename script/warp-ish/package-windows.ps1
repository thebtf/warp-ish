#requires -Version 7.0
<# Builds a local Windows x64 candidate. Does not install or publish it. #>
param(
    [Parameter(Mandatory)]
    [ValidatePattern('\A[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?\z')]
    [string]$Version
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'X64') {
    throw 'This workflow requires Windows x64 and PowerShell 7.'
}
foreach ($name in @('GIT_DIR', 'GIT_COMMON_DIR', 'GIT_WORK_TREE', 'GIT_INDEX_FILE')) {
    if ($null -ne [Environment]::GetEnvironmentVariable($name, 'Process')) {
        throw "Unset $name before packaging. The checkout determines the Git source."
    }
}

function Invoke-Git([string[]]$Arguments, [string]$Checkout = $repo) {
    $result = & git -C $Checkout @Arguments
    if ($LASTEXITCODE -ne 0) { throw "git failed: $Arguments" }
    return ($result -join "`n").Trim()
}

function Get-SourceFingerprint {
    # Tracked edits always block, even if a runtime root were accidentally tracked.
    $tracked = Invoke-Git @('status', '--porcelain', '--untracked-files=no')
    # Session-owned state is not a product input. Keep all other untracked paths visible.
    $source = Invoke-Git @('status', '--porcelain', '--untracked-files=normal', '--', '.',
        ':(top,exclude,glob).agent/continuity/**',
        ':(top,exclude,glob).agent/session-checkpoints/**',
        ':(top,exclude,glob).agent/tmp/**',
        ':(top,exclude,glob).serena/**')
    if ($tracked -or $source) {
        throw 'Commit source changes before packaging; checkout must be clean.'
    }
    return (Invoke-Git @('rev-parse', 'HEAD')) + ':' + (Invoke-Git @('rev-parse', 'HEAD^{tree}'))
}

function Assert-NoReparsePoint([string]$Path) {
    for ($item = [IO.DirectoryInfo]::new($Path); $item; $item = $item.Parent) {
        try {
            $existing = Get-Item -LiteralPath $item.FullName -Force
        } catch [System.Management.Automation.ItemNotFoundException] { continue }
        if ($existing.Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw "Reparse point in package path: $($item.FullName)"
        }
    }
}

function Assert-SafePath([string]$Path, [string]$Root) {
    $full = [IO.Path]::GetFullPath($Path)
    if (-not $full.StartsWith($Root + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Path escapes $Root : $full"
    }
    Assert-NoReparsePoint $full
    return $full
}

function Get-GitPath([string]$Argument, [string]$Checkout = $repo) {
    $path = Invoke-Git @('rev-parse', '--path-format=absolute', $Argument) $Checkout
    if (-not [IO.Path]::IsPathFullyQualified($path)) { throw "Git returned a non-absolute $Argument path." }
    return [IO.Path]::GetFullPath($path)
}

function Get-OperationsDirectory {
    Assert-NoReparsePoint $repo
    if ((Get-GitPath '--show-toplevel') -ne $repo) { throw 'Package source must be a Git checkout root.' }
    $common = Get-GitPath '--git-common-dir'
    $gitDir = Get-GitPath '--git-dir'
    if ([IO.Path]::GetFileName($common) -ne '.git') { throw "Unexpected Git common directory: $common" }
    Assert-NoReparsePoint $common
    Assert-NoReparsePoint $gitDir
    if (-not (Test-Path -LiteralPath $common -PathType Container) -or
        -not (Test-Path -LiteralPath $gitDir -PathType Container)) { throw 'Git metadata directory is missing.' }
    $main = [IO.Path]::GetDirectoryName($common)
    if ($repo -eq $main) {
        if ($gitDir -ne $common) { throw 'Main checkout Git metadata does not match its common directory.' }
    } else {
        $taskParent = [IO.Path]::GetDirectoryName($repo)
        if ($taskParent -notin @((Join-Path $main '.work\wt'), (Join-Path $main '.agent\worktrees')) -or
            [IO.Path]::GetDirectoryName($gitDir) -ne (Join-Path $common 'worktrees') -or
            (Get-GitPath '--show-toplevel' $main) -ne $main -or
            (Get-GitPath '--git-common-dir' $main) -ne $common) {
            throw "Checkout is not a managed worktree of its Git common directory: $repo"
        }
        $backlink = Join-Path $gitDir 'gitdir'
        $checkoutGitFile = Join-Path $repo '.git'
        Assert-RegularFile $backlink $gitDir
        Assert-RegularFile $checkoutGitFile $repo
        $registeredGitFile = [IO.Path]::GetFullPath(([IO.File]::ReadAllText($backlink).Trim()), $gitDir)
        if ($registeredGitFile -ne $checkoutGitFile) {
            throw "Git worktree backlink does not identify this checkout: $repo"
        }
    }
    return Assert-SafePath (Join-Path $main '.agent') $main
}

function Assert-RegularFile([string]$Path, [string]$Root, [switch]$AllowGeneratedHardLink) {
    [void](Assert-SafePath $Path $Root)
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.PSIsContainer -or $item.Length -eq 0 -or
        ($item.LinkType -eq 'HardLink' -and -not $AllowGeneratedHardLink)) {
        throw "Expected nonempty regular file: $Path"
    }
    $stream = [IO.File]::OpenRead($Path)
    try {
        $prefix = [byte[]]::new(80)
        $length = $stream.Read($prefix, 0, $prefix.Length)
        if ([Text.Encoding]::ASCII.GetString($prefix, 0, $length).StartsWith('version https://git-lfs.github.com/spec/')) {
            throw "Unresolved LFS asset: $Path"
        }
    } finally { $stream.Dispose() }
}

function Write-Json([string]$Path, $Value) {
    $Value | ConvertTo-Json -Depth 16 | Set-Content -LiteralPath $Path -Encoding utf8NoBOM
}

function Invoke-Logged([string]$Command, [string[]]$Arguments) {
    "COMMAND: $Command $($Arguments -join ' ')" | Add-Content -LiteralPath $log
    & $Command @Arguments 2>&1 | ForEach-Object {
        Add-Content -LiteralPath $log -Value $_
        Write-Host $_
    }
    if ($LASTEXITCODE -ne 0) { throw "$Command exited $LASTEXITCODE; see $log" }
}

$operations = Get-OperationsDirectory
# Reuse the owning release-tool pin rather than maintaining another version.
$pinSource = Join-Path $repo 'script\install_cargo_release_deps'
$pinMatch = [regex]::Match((Get-Content -LiteralPath $pinSource -Raw), '(?m)^CARGO_ABOUT_VERSION="([0-9.]+)"\r?$')
if (-not $pinMatch.Success) { throw 'Cannot read the cargo-about release pin.' }
$aboutVersion = & cargo-about --version
if ($LASTEXITCODE -ne 0 -or $aboutVersion -ne "cargo-about $($pinMatch.Groups[1].Value)") {
    throw "Install cargo-about $($pinMatch.Groups[1].Value) using script/install_cargo_release_deps before packaging."
}
$fingerprint = Get-SourceFingerprint
$sourceSha, $sourceTree = $fingerprint.Split(':')
$policyPath = Join-Path $repo 'script\warp-ish\terminal-policy.json'
$policy = Get-Content -LiteralPath $policyPath -Raw | ConvertFrom-Json
$upstreamSha = $policy.observed_upstream_sha
if ($upstreamSha -notmatch '^[a-f0-9]{40}$') { throw 'Policy has no immutable upstream SHA.' }
[void](Invoke-Git @('merge-base', '--is-ancestor', $upstreamSha, $sourceSha))
$toolchainFile = Join-Path $repo 'rust-toolchain.toml'
$toolchainMatch = [regex]::Matches((Get-Content -LiteralPath $toolchainFile -Raw),
    '(?m)^channel\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"\s*\r?$')
if ($toolchainMatch.Count -ne 1) { throw 'Packaging requires one immutable Rust version in rust-toolchain.toml.' }
$toolchain = $toolchainMatch[0].Groups[1].Value
Push-Location -LiteralPath $repo
try {
    # rustup's command-line selection outranks environment/directory overrides.
    $cargoVersion = (& cargo "+$toolchain" --version) -join "`n"
    if ($LASTEXITCODE -ne 0 -or $cargoVersion -notmatch ("\Acargo " + [regex]::Escape($toolchain) + '(?:\s|\z)')) {
        throw "cargo does not match repository toolchain $toolchain."
    }
    $rustVersion = (& rustc "+$toolchain" -vV) -join "`n"
    if ($LASTEXITCODE -ne 0 -or $rustVersion -notmatch ("\Arustc " + [regex]::Escape($toolchain) + '(?:\s|\z)') -or
        $rustVersion -notmatch '(?m)^host: x86_64-pc-windows-msvc$') {
        throw "rustc does not match repository toolchain $toolchain with an x64 MSVC host."
    }
} finally { Pop-Location }

$runId = "package-$Version-$([guid]::NewGuid().ToString('N'))"
$run = Assert-SafePath (Join-Path $operations "runs\$runId") $operations
$target = Assert-SafePath (Join-Path $operations "build\$runId") $operations
$packages = Assert-SafePath (Join-Path $operations 'build\packages') $operations
$destination = Assert-SafePath (Join-Path $packages $Version) $packages
$stage = Assert-SafePath (Join-Path $packages ".$runId.incomplete") $packages
$assets = Join-Path $repo 'app\assets\windows\x64'
$payloadSources = [ordered]@{
    'conpty.dll' = (Join-Path $assets 'conpty.dll')
    'x64\OpenConsole.exe' = (Join-Path $assets 'OpenConsole.exe')
    'vcruntime140.dll' = (Join-Path $assets 'vcruntime140.dll')
    'vcruntime140_1.dll' = (Join-Path $assets 'vcruntime140_1.dll')
    'msvcp140.dll' = (Join-Path $assets 'msvcp140.dll')
    'dxcompiler.dll' = (Join-Path $assets 'dxcompiler.dll')
    'dxil.dll' = (Join-Path $assets 'dxil.dll')
    'pwsh.ps1' = (Join-Path $repo 'app\assets\bundled\bootstrap\pwsh.ps1')
    'icon.ico' = (Join-Path $repo 'app\channels\oss\icon\no-padding\icon.ico')
}
foreach ($path in $payloadSources.Values) { Assert-RegularFile $path $repo }
$log = Join-Path $run 'build.log'
$statePath = Join-Path $run 'state.json'
$state = [ordered]@{ status = 'building'; source_sha = $sourceSha; version = $Version; stage = $stage; target = $target }
$envNames = @('CARGO_FULL_PROFILE', 'CARGO_BIN_NAME', 'WARP_APP_NAME', 'GIT_RELEASE_TAG',
    'CARGO_INCREMENTAL', 'CARGO_TARGET_DIR', 'RUSTUP_AUTO_INSTALL', 'SKIP_SETTINGS_SCHEMA',
    'SETTINGS_SCHEMA_SOURCE', 'SETTINGS_SCHEMA_EXECUTABLE', 'RUSTUP_TOOLCHAIN')
$savedEnv = @{}
foreach ($name in $envNames) { $savedEnv[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
$buildArguments = @("+$toolchain", 'build', '-p', 'warp', '--bin', 'warp-oss', '--profile', 'rlto',
    '--features', 'release_bundle,gui', '--target', 'x86_64-pc-windows-msvc',
    '--target-dir', $target, '--locked', '-j', '2')
$promoted = $false
$started = $false
$claimed = $false
# The fixed side-output directory belongs to one packaging process per checkout.
$claimKey = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($repo.ToUpperInvariant())))
$claim = [Threading.Mutex]::new($false, "Global\warp-ish-package-$claimKey")
Push-Location -LiteralPath $repo
try {
    try { $claimed = $claim.WaitOne(0) }
    catch [Threading.AbandonedMutexException] { $claimed = $true }
    if (-not $claimed) { throw "Another packaging process already owns this checkout: $repo" }
    $sideOutput = Assert-SafePath (Join-Path $repo 'target\rlto') $repo
    foreach ($path in @($run, $target, $sideOutput, $destination, $stage)) {
        if (Test-Path -LiteralPath $path) { throw "Refusing to overwrite existing path: $path" }
    }
    [void][IO.Directory]::CreateDirectory($run)
    $started = $true
    [void][IO.Directory]::CreateDirectory($stage)
    [void][IO.Directory]::CreateDirectory((Join-Path $stage 'app\x64'))
    [void][IO.Directory]::CreateDirectory($target)
    # app/build.rs writes here even when Cargo uses --target-dir elsewhere.
    [void][IO.Directory]::CreateDirectory($sideOutput)
    Write-Json $statePath $state

    $env:CARGO_FULL_PROFILE = 'rlto'
    $env:CARGO_BIN_NAME = 'oss'
    $env:WARP_APP_NAME = 'warp-ish'
    $env:GIT_RELEASE_TAG = $Version
    $env:CARGO_INCREMENTAL = '0'
    $env:CARGO_TARGET_DIR = $target
    $env:RUSTUP_AUTO_INSTALL = '0'
    $env:RUSTUP_TOOLCHAIN = $toolchain
    Invoke-Logged 'cargo' $buildArguments
    if ((Get-SourceFingerprint) -ne $fingerprint) { throw 'Source changed during build.' }
    $binary = Join-Path $target 'x86_64-pc-windows-msvc\rlto\warp-oss.exe'
    # Cargo can hard-link its executable to the matching artifact in deps.
    Assert-RegularFile $binary $target -AllowGeneratedHardLink
    $packagedBinary = Join-Path $stage 'app\warp-ish.exe'
    [IO.File]::Copy($binary, $packagedBinary)
    Assert-RegularFile $packagedBinary $stage
    foreach ($entry in $payloadSources.GetEnumerator()) {
        Copy-Item -LiteralPath $entry.Value -Destination (Join-Path $stage "app\$($entry.Key)")
    }
    $env:SKIP_SETTINGS_SCHEMA = '0'
    $env:SETTINGS_SCHEMA_SOURCE = $null
    $env:SETTINGS_SCHEMA_EXECUTABLE = Join-Path $stage 'app\warp-ish.exe'
    # Isolate the existing script's exit statements; this executes only the schema CLI.
    Invoke-Logged (Join-Path $PSHOME 'pwsh.exe') @('-NoProfile', '-File',
        (Join-Path $repo 'script\windows\prepare_bundled_resources.ps1'),
        '-DestinationDir', (Join-Path $stage 'app\resources'), '-Channel', 'oss', '-TerminalOnly')
    $schema = Get-Content -LiteralPath (Join-Path $stage 'app\resources\settings_schema.json') -Raw | ConvertFrom-Json
    if ($null -eq $schema -or $schema -is [array] -or $schema -is [string]) { throw 'Invalid settings schema object.' }
    $metadata = Get-Content -LiteralPath (Join-Path $stage 'app\resources\bundled\metadata\version.json') -Raw | ConvertFrom-Json
    if ($metadata.warp_version -ne $Version) { throw 'Resource version mismatch.' }
    $app = Join-Path $stage 'app'
    $expected = @('warp-ish.exe') + @($payloadSources.Keys) + @('resources\THIRD_PARTY_LICENSES.txt',
        'resources\settings_schema.json', 'resources\bundled\metadata\version.json')
    $actual = @(Get-ChildItem -LiteralPath $app -File -Recurse -Force | ForEach-Object { [IO.Path]::GetRelativePath($app, $_.FullName) })
    if (Compare-Object ($expected | Sort-Object) ($actual | Sort-Object)) { throw 'Unexpected package payload.' }
    $expectedDirectories = @('x64', 'resources', 'resources\bundled', 'resources\bundled\metadata')
    $directories = @(Get-ChildItem -LiteralPath $app -Directory -Recurse -Force | ForEach-Object {
        [void](Assert-SafePath $_.FullName $stage)
        [IO.Path]::GetRelativePath($app, $_.FullName)
    })
    if (Compare-Object ($expectedDirectories | Sort-Object) ($directories | Sort-Object)) { throw 'Unexpected package directories.' }
    $hashes = @($actual | Sort-Object | ForEach-Object {
        $path = Join-Path $app $_
        Assert-RegularFile $path $stage
        [ordered]@{ path = $_.Replace('\', '/'); size = (Get-Item -LiteralPath $path).Length;
            sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() }
    })
    if ((Get-SourceFingerprint) -ne $fingerprint) { throw 'Source changed during resource preparation.' }
    $manifest = [ordered]@{
        schema_version = 1; version = $Version; upstream_sha = $upstreamSha; fork_sha = $sourceSha
        source_tree = $sourceTree; source_repository = $policy.source_repository
        cargo_lock_sha256 = (Get-FileHash -LiteralPath (Join-Path $repo 'Cargo.lock')).Hash.ToLowerInvariant()
        toolchain_file_sha256 = (Get-FileHash -LiteralPath (Join-Path $repo 'rust-toolchain.toml')).Hash.ToLowerInvariant()
        cargo = $cargoVersion; rustc = $rustVersion; cargo_about = $aboutVersion; toolchain = $toolchain
        rustflags = $env:RUSTFLAGS; cargo_encoded_rustflags = $env:CARGO_ENCODED_RUSTFLAGS
        build_arguments = $buildArguments; schema_executable = 'app/warp-ish.exe'; files = $hashes
        acceptance = 'NOT_RUN'; signed = $false
    }
    Write-Json (Join-Path $stage 'manifest.json') $manifest
    # Archive only application payload; sibling manifest/report bind it without self-reference.
    $zip = Join-Path $stage "warp-ish-$Version-windows-x64.zip"
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [IO.Compression.ZipFile]::CreateFromDirectory($app, $zip, [IO.Compression.CompressionLevel]::Optimal, $true)
    $archive = [IO.Compression.ZipFile]::OpenRead($zip)
    try {
        $entries = @($archive.Entries | Where-Object { $_.Name } | ForEach-Object { $_.FullName.Replace('\', '/') })
        if (Compare-Object ($hashes | ForEach-Object { 'app/' + $_.path }) $entries) { throw 'ZIP payload mismatch.' }
        foreach ($file in $hashes) {
            $entry = $archive.GetEntry('app/' + $file.path)
            $stream = $entry.Open()
            $sha = [Security.Cryptography.SHA256]::Create()
            try { $digest = [Convert]::ToHexString($sha.ComputeHash($stream)).ToLowerInvariant() }
            finally { $stream.Dispose(); $sha.Dispose() }
            if ($digest -ne $file.sha256) { throw "ZIP hash mismatch: $($file.path)" }
        }
    } finally { $archive.Dispose() }
    Write-Json (Join-Path $stage 'package.json') ([ordered]@{
        version = $Version; fork_sha = $sourceSha; upstream_sha = $upstreamSha
        archive = [IO.Path]::GetFileName($zip); archive_sha256 = (Get-FileHash -LiteralPath $zip).Hash.ToLowerInvariant()
        manifest_sha256 = (Get-FileHash -LiteralPath (Join-Path $stage 'manifest.json')).Hash.ToLowerInvariant()
    })
    if ((Get-SourceFingerprint) -ne $fingerprint) { throw 'Source changed before promotion.' }
    [void](Assert-SafePath $stage $packages)
    [void](Assert-SafePath $destination $packages)
    [IO.Directory]::Move($stage, $destination)
    $promoted = $true
    $state.status = 'packaged_not_accepted'
    $state['package'] = $destination
} catch {
    if (-not $started) { throw }
    $failure = $_
    $state.status = 'failed'
    $state['error'] = $_.Exception.Message
    try {
        $state['failed_stage_files'] = @(Get-ChildItem -LiteralPath $stage -File -Recurse -Force | ForEach-Object {
            [void](Assert-SafePath $_.FullName $stage)
            [ordered]@{ path = [IO.Path]::GetRelativePath($stage, $_.FullName); size = $_.Length;
                sha256 = (Get-FileHash -LiteralPath $_.FullName).Hash.ToLowerInvariant() }
        })
    } catch { $state['failure_evidence_error'] = $_.Exception.Message }
    Write-Json $statePath $state
    throw $failure
} finally {
    # Nothing uses Cargo outputs after schema/ZIP verification. Preserve package and run logs.
    try {
        if ($started) {
            $cleanupErrors = @()
            $cleanupRoots = [ordered]@{ $target = $operations; $sideOutput = $repo }
            if (-not $promoted) { $cleanupRoots[$stage] = $packages }
            foreach ($path in $cleanupRoots.Keys) {
                try {
                    [void](Assert-SafePath $path $cleanupRoots[$path])
                    if (Test-Path -LiteralPath $path) {
                        foreach ($child in Get-ChildItem -LiteralPath $path -Force -Recurse) {
                            [void](Assert-SafePath $child.FullName $path)
                        }
                        Remove-Item -LiteralPath $path -Recurse -Force
                    }
                } catch {
                    $cleanupErrors += [ordered]@{ retained_path = $path; error = $_.Exception.Message }
                }
            }
            $state['build_targets_cleaned'] = -not ((Test-Path -LiteralPath $target) -or (Test-Path -LiteralPath $sideOutput))
            if (-not $promoted) { $state['failed_stage_removed'] = -not (Test-Path -LiteralPath $stage) }
            if ($cleanupErrors.Count) {
                $state['cleanup_errors'] = $cleanupErrors
                throw "Cleanup refused or failed; retained paths and reasons: $($cleanupErrors | ConvertTo-Json -Compress)"
            }
        }
    } finally {
        try {
            foreach ($name in $envNames) { [Environment]::SetEnvironmentVariable($name, $savedEnv[$name], 'Process') }
            Pop-Location
            if ($started) { Write-Json $statePath $state }
        } finally {
            try { if ($claimed) { $claim.ReleaseMutex() } }
            finally { $claim.Dispose() }
        }
    }
}
if ($promoted) { Write-Output "Local candidate: $destination (acceptance NOT_RUN); evidence: $run" }
