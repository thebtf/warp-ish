#
# Prepares bundled resources for distribution on Windows.
#
# This script copies resources that should be bundled with Warp into a
# destination directory. It is used by the Windows build script.
#
# Usage:
#   prepare_bundled_resources.ps1 <destination_directory>
#
# Arguments:
#   destination_directory: The directory where resources should be installed.
#                          Resources will be copied to subdirectories within
#                          this path (e.g., $DEST_DIR\skills).

Param(
    [Parameter(Mandatory = $true)]
    [String]$DestinationDir,

    [Parameter(Mandatory = $false)]
    [String]$Channel = '',

    [Switch]$TerminalOnly
)

$ErrorActionPreference = 'Stop'

$ScriptDir = Split-Path -Path $MyInvocation.MyCommand.Path -Parent
$RepoRoot = (Get-Item -LiteralPath "$ScriptDir\..\.." | Select-Object -ExpandProperty FullName)
$ResourcesSource = Join-Path $RepoRoot 'resources'

# Validate that the source resources directory exists
if (-Not (Test-Path -LiteralPath $ResourcesSource -PathType Container)) {
    Write-Error "Resources directory not found at $ResourcesSource"
    exit 1
}

if ($TerminalOnly) {
    if (-not $env:GIT_RELEASE_TAG -or $env:SKIP_SETTINGS_SCHEMA -eq '1') {
        throw 'warp-ish bundles require a release tag and a settings schema.'
    }

    $provider = $null
    $drive = $null
    $DestinationDir = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath(
        $DestinationDir, [ref]$provider, [ref]$drive
    )
    if ($provider.Name -ne 'FileSystem') {
        throw 'warp-ish resources must be staged on the filesystem.'
    }

    for ($directory = [IO.DirectoryInfo]::new($DestinationDir); $directory; $directory = $directory.Parent) {
        if (Test-Path -LiteralPath $directory.FullName) {
            $item = Get-Item -LiteralPath $directory.FullName -Force
            if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
                throw "Unsafe warp-ish resource directory: $($directory.FullName)"
            }
        }
    }

    $allowedDirectories = @('bundled', 'bundled\metadata')
    $allowedFiles = @('THIRD_PARTY_LICENSES.txt', 'settings_schema.json', 'bundled\metadata\version.json')
    foreach ($relativeDir in @('') + $allowedDirectories) {
        $directory = Join-Path $DestinationDir $relativeDir
        if (Test-Path -LiteralPath $directory -PathType Container) {
            foreach ($item in Get-ChildItem -LiteralPath $directory -Force) {
                $relativePath = if ($relativeDir) { Join-Path $relativeDir $item.Name } else { $item.Name }
                $allowed = if ($item.PSIsContainer) { $allowedDirectories } else { $allowedFiles }
                if ($relativePath -notin $allowed -or $item.LinkType -eq 'HardLink' -or
                    ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
                    throw "Unexpected warp-ish resource: $($item.FullName). Use a clean staging directory."
                }
            }
        }
    }
    [void][IO.Directory]::CreateDirectory((Join-Path $DestinationDir 'bundled\metadata'))
} elseif (-Not (Test-Path $DestinationDir)) {
    New-Item -ItemType Directory -Path $DestinationDir -Force | Out-Null
}

# Copy bundled resources
$BundledSource = Join-Path $ResourcesSource 'bundled'
if (-not $TerminalOnly -and (Test-Path $BundledSource -PathType Container)) {
    $BundledDestination = Join-Path $DestinationDir 'bundled'
    Write-Output "Copying bundled resources to $BundledDestination"
    if (Test-Path $BundledDestination -PathType Container) {
        Remove-Item -Path $BundledDestination -Recurse -Force
    }
    Copy-Item -Path $BundledSource -Destination $BundledDestination -Recurse -Force
} elseif (-not $TerminalOnly) {
    Write-Warning "No bundled directory found at $BundledSource"
}

if ($env:GIT_RELEASE_TAG) {
    $VersionMetadataDir = Join-Path (Join-Path $DestinationDir 'bundled') 'metadata'
    $VersionMetadataPath = Join-Path $VersionMetadataDir 'version.json'
    Write-Output "Writing bundled Warp version metadata to $VersionMetadataPath"
    if (-Not (Test-Path -LiteralPath $VersionMetadataDir -PathType Container)) {
        New-Item -ItemType Directory -Path $VersionMetadataDir -Force | Out-Null
    }

    @{ warp_version = $env:GIT_RELEASE_TAG } |
        ConvertTo-Json |
        Set-Content -LiteralPath $VersionMetadataPath -Encoding utf8
}

# Copy channel-gated skills matching the current release channel.
$GatedSource = Join-Path (Join-Path $RepoRoot 'resources') 'channel-gated-skills'
$DestSkills = Join-Path (Join-Path $DestinationDir 'bundled') 'skills'

if (-not $TerminalOnly -and $Channel -and (Test-Path $GatedSource -PathType Container)) {
    Write-Output "Copying channel-gated skills for channel '$Channel'..."

    # Error out if a stable/ gate directory exists.
    $StableDir = Join-Path $GatedSource 'stable'
    if (Test-Path $StableDir -PathType Container) {
        Write-Error "Found a 'stable/' directory in $GatedSource. The stable channel does not use gated skills. Move stable-ready skills to resources/skills/ instead."
        exit 1
    }

    # Gate labels ordered from most-inclusive to least-inclusive.
    $GateOrder = @('dogfood', 'preview')

    # Map the release channel to its gate label.
    switch ($Channel) {
        'local' { $Gate = 'dogfood' }
        'dev' { $Gate = 'dogfood' }
        'preview' { $Gate = 'preview' }
        default {
            Write-Output "  Channel '$Channel' has no gated skills, skipping"
            $Gate = $null
        }
    }

    if ($Gate) {
        # Build the set of included gates (progressive: this gate and all after it).
        $GateIndex = [array]::IndexOf($GateOrder, $Gate)
        $IncludedGates = $GateOrder[$GateIndex..($GateOrder.Length - 1)]

        foreach ($GateDir in Get-ChildItem -Path $GatedSource -Directory | Sort-Object Name) {
            if ($GateDir.Name -notin $IncludedGates) {
                $Skills = (Get-ChildItem -Path $GateDir.FullName -Directory | Sort-Object Name | ForEach-Object { $_.Name }) -join ', '
                Write-Output "  Skipping gate '$($GateDir.Name)' (channel '$Channel') - would include: $Skills"
                continue
            }

            foreach ($SkillDir in Get-ChildItem -Path $GateDir.FullName -Directory | Sort-Object Name) {
                $Dest = Join-Path $DestSkills $SkillDir.Name
                Write-Output "  Copying gated skill: $($SkillDir.Name) (gate: $($GateDir.Name))"
                Copy-Item -Path $SkillDir.FullName -Destination $Dest -Recurse -Force
            }
        }
    }
}

# Generate third-party license attribution.
#
# Additional (non-Cargo) third-party license files to include in the output.
# When adding a new third-party component to the bundle, add its license file
# to the repo alongside the component and add an entry here.
# Cross-platform components:
$AdditionalLicenses = @(
    @{ Name = 'Alacritty (alacritty_terminal)'; License = 'Apache-2.0'; Path = 'crates\warp_terminal\src\model\LICENSE-ALACRITTY' },
    @{ Name = 'Hack Font'; License = 'MIT'; Path = 'app\assets\bundled\fonts\hack\LICENSE.md' },
    @{ Name = 'Roboto Font'; License = 'SIL Open Font License'; Path = 'app\assets\bundled\fonts\roboto\LICENSE.txt' },
    @{ Name = 'bash-preexec'; License = 'MIT'; Path = 'app\assets\bundled\bootstrap\bash-preexec-LICENSE.md' }
)
if (-not $TerminalOnly) {
    $AdditionalLicenses += @{ Name = 'Claude API Skill'; License = 'Apache-2.0'; Path = 'resources\bundled\skills\claude-api\LICENSE.txt' }
}
$AdditionalLicenses += @(
    @{ Name = 'Windows Terminal'; License = 'MIT'; Path = 'app\assets\windows\LICENSE-WINDOWS-TERMINAL' },
    @{ Name = 'GitHub Desktop'; License = 'MIT'; Path = 'app\src\code_review\GITHUB-DESKTOP-LICENSE' }
)
# Windows-only components:
$AdditionalLicenses += @(
    @{ Name = 'OpenConsole / ConPTY (Windows Terminal)'; License = 'MIT'; Path = 'app\assets\windows\LICENSE-WINDOWS-TERMINAL' },
    @{ Name = 'DirectX Shader Compiler'; License = 'NCSA'; Path = 'app\assets\windows\LICENSE-DXC' }
)

$LicensesOutput = Join-Path $DestinationDir 'THIRD_PARTY_LICENSES.txt'
Write-Output "Generating third-party licenses at $LicensesOutput"
if ($TerminalOnly) {
    $manifest = Join-Path $RepoRoot 'app\Cargo.toml'
    cargo about generate --locked --manifest-path $manifest --features 'release_bundle gui' --target x86_64-pc-windows-msvc -c "$RepoRoot\about.toml" -o $LicensesOutput "$RepoRoot\about.hbs"
} else {
    cargo about generate --workspace --manifest-path "$RepoRoot\Cargo.toml" -c "$RepoRoot\about.toml" -o "$LicensesOutput" "$RepoRoot\about.hbs"
}
if ($LASTEXITCODE -ne 0) {
    Write-Error 'Failed to generate third-party licenses'
    exit 1
}

# Append additional (non-Cargo) third-party licenses.
foreach ($entry in $AdditionalLicenses) {
    $LicenseFile = Join-Path $RepoRoot $entry.Path
    if (-Not (Test-Path -LiteralPath $LicenseFile)) {
        Write-Error "License file not found: $LicenseFile"
        exit 1
    }
    Add-Content -LiteralPath $LicensesOutput -Value ''
    Add-Content -LiteralPath $LicensesOutput -Value "$($entry.Name) ($($entry.License))"
    Add-Content -LiteralPath $LicensesOutput -Value ('-' * 80)
    Get-Content -LiteralPath $LicenseFile | Add-Content -LiteralPath $LicensesOutput
    Add-Content -LiteralPath $LicensesOutput -Value ''
}

# Generate settings JSON schema unless explicitly skipped.
if ($env:SKIP_SETTINGS_SCHEMA -ne '1') {
    $SchemaOutput = Join-Path $DestinationDir 'settings_schema.json'

    if ($env:SETTINGS_SCHEMA_EXECUTABLE -and $env:SETTINGS_SCHEMA_SOURCE) {
        Write-Error 'SETTINGS_SCHEMA_EXECUTABLE and SETTINGS_SCHEMA_SOURCE are mutually exclusive.'
        exit 1
    } elseif ($env:SETTINGS_SCHEMA_EXECUTABLE) {
        if (-Not (Test-Path -LiteralPath $env:SETTINGS_SCHEMA_EXECUTABLE -PathType Leaf)) {
            Write-Error "Settings schema executable does not exist: $env:SETTINGS_SCHEMA_EXECUTABLE"
            exit 1
        }
        Write-Output "Generating settings schema at $SchemaOutput"
        $Process = Start-Process `
            -FilePath $env:SETTINGS_SCHEMA_EXECUTABLE `
            -ArgumentList @('dump-settings-schema', ('"{0}"' -f $SchemaOutput)) `
            -NoNewWindow `
            -PassThru `
            -Wait
        if ($Process.ExitCode -ne 0) {
            Write-Error "Failed to generate settings schema: executable exited with code $($Process.ExitCode)"
            exit 1
        }
    } elseif ($env:SETTINGS_SCHEMA_SOURCE) {
        if (-Not (Test-Path -LiteralPath $env:SETTINGS_SCHEMA_SOURCE -PathType Leaf)) {
            Write-Error "Settings schema source does not exist: $env:SETTINGS_SCHEMA_SOURCE"
            exit 1
        }
        Write-Output "Copying settings schema to $SchemaOutput"
        Copy-Item -LiteralPath $env:SETTINGS_SCHEMA_SOURCE -Destination $SchemaOutput -Force
    } else {
        Write-Error 'Set SETTINGS_SCHEMA_EXECUTABLE or SETTINGS_SCHEMA_SOURCE when preparing a settings schema.'
        exit 1
    }

    if (-Not (Test-Path -LiteralPath $SchemaOutput -PathType Leaf) -or (Get-Item -LiteralPath $SchemaOutput).Length -eq 0) {
        Write-Error "Settings schema was not written to $SchemaOutput"
        exit 1
    }
}

Write-Output "Successfully prepared bundled resources in $DestinationDir"
