#requires -Version 7.4
[CmdletBinding()]
param(
    [string]$PackageDir = $env:WARP_ISH_MCP_PACKAGE,
    [string]$PythonExe
)

$ErrorActionPreference = 'Stop'
if (!$IsWindows) { throw 'MCP package acceptance requires Windows.' }
$checkout = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$commonDir = & git -C $checkout rev-parse --path-format=absolute --git-common-dir
if ($LASTEXITCODE) { throw 'Could not resolve the shared project directory.' }
$root = Split-Path $commonDir -Parent
if (!$PackageDir) {
    $packages = Join-Path $root '.agent/build/packages'
    $candidates = @(Get-ChildItem -LiteralPath $packages -Directory |
        Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName 'app/warp-ish-mcp.exe') })
    if ($candidates.Count -ne 1) {
        throw 'Specify -PackageDir or WARP_ISH_MCP_PACKAGE for one packaged MCP candidate.'
    }
    $PackageDir = $candidates[0].FullName
}
$PackageDir = (Resolve-Path -LiteralPath $PackageDir).Path
foreach ($binary in 'app/warp-ish.exe','app/warp-ish-mcp.exe') {
    if (!(Test-Path -LiteralPath (Join-Path $PackageDir $binary) -PathType Leaf)) {
        throw "Missing packaged executable $binary"
    }
}
if (!$PythonExe) {
    $PythonExe = (Get-Command python -CommandType Application -ErrorAction Stop |
        Select-Object -First 1).Source
}
Add-Type -AssemblyName System.Security.Cryptography.ProtectedData
$setting = [System.Security.Cryptography.ProtectedData]::Protect(
    [Text.Encoding]::UTF8.GetBytes('"Enabled"'), $null,
    [System.Security.Cryptography.DataProtectionScope]::CurrentUser
)
$run = 'mcp-windows-' + [Guid]::NewGuid().ToString('n')
$scratch = Join-Path $root ".agent/tmp/$run"
$evidence = Join-Path $root ".agent/runs/$run"
[void][IO.Directory]::CreateDirectory($scratch)
[void][IO.Directory]::CreateDirectory($evidence)
$seed = Join-Path $scratch 'scripting.dpapi'
[IO.File]::WriteAllBytes($seed, $setting)
$profileRoot = [Environment]::GetFolderPath([Environment+SpecialFolder]::UserProfile)
$roaming = [Environment]::GetFolderPath([Environment+SpecialFolder]::ApplicationData)
$local = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
if (!$profileRoot -or !$roaming -or !$local) { throw 'Windows profile folders could not be resolved.' }
& $PythonExe -B (Join-Path $PSScriptRoot 'mcp-windows-acceptance.py') $PackageDir $scratch $evidence $seed $profileRoot $roaming $local
exit $LASTEXITCODE
