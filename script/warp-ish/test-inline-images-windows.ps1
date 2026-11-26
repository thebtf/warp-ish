#requires -Version 7.4
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$PackageDir,
    [ValidateSet('control','kitty','iterm','sixel','kitty-multipart','iterm-multipart','kitty-virtual','kitty-alt','kitty-crop-alt','iterm-alt','sixel-alt','sixel-clip-alt','omp','omp-iterm2','omp-sixel','omp-kitty')]
    [string[]]$Cases = @('control','kitty','iterm','sixel','kitty-multipart','iterm-multipart','kitty-virtual','kitty-alt','kitty-crop-alt','iterm-alt','sixel-alt','sixel-clip-alt','omp'),
    [string]$OmpExe = (Get-Command omp -CommandType Application | Select-Object -First 1).Source
)
$ErrorActionPreference = 'Stop'
$package = (Resolve-Path -LiteralPath $PackageDir).Path
$omp = (Resolve-Path -LiteralPath $OmpExe).Path
if (-not (Test-Path -LiteralPath (Join-Path $package 'app/warp-ish.exe') -PathType Leaf)) {
    throw 'Expected a packaged warp-ish executable.'
}
$run = [Guid]::NewGuid().ToString('n')
$common = & git -C $PSScriptRoot rev-parse --path-format=absolute --git-common-dir
if ($LASTEXITCODE) { throw 'Could not locate the shared project directory.' }
$root = Split-Path $common -Parent
$work = Join-Path $root ".agent/tmp/inline-image-work-$run"
$evidence = Join-Path $root ".agent/runs/inline-image-result-$run"
$python = (Get-Command python -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
& $python -I -c 'from PIL import Image, ImageChops'
if ($LASTEXITCODE) { throw 'Pillow is required for Windows image acceptance.' }
[void][IO.Directory]::CreateDirectory($work)
[void][IO.Directory]::CreateDirectory($evidence)
Write-Host $evidence
Add-Type -AssemblyName System.Security.Cryptography.ProtectedData
$setting = [System.Security.Cryptography.ProtectedData]::Protect(
    [Text.Encoding]::UTF8.GetBytes('"Enabled"'), $null, [System.Security.Cryptography.DataProtectionScope]::CurrentUser)
$seed = Join-Path $work 'seed'
[IO.File]::WriteAllBytes($seed, $setting)
& $python -I -B (Join-Path $PSScriptRoot 'inline-image-windows.py') $package $omp $work $evidence $seed ([Environment]::GetFolderPath('UserProfile')) ([Environment]::GetFolderPath('ApplicationData')) ([Environment]::GetFolderPath('LocalApplicationData')) @Cases
exit $LASTEXITCODE
