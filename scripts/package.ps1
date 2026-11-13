# QuickShot Windows packaging script — produces
# a portable ZIP and a per-user Setup.exe from the same release binary.
# Requires NSIS 3 (makensis.exe).
#
# Requirements when running locally:
#   - PowerShell 5+ / pwsh
#   - Rust toolchain with x86_64-pc-windows-msvc target
#   - Visual Studio Build Tools (link.exe / cl.exe). The script does NOT
#     invoke vcvars64.bat — invoke this script from a Developer PowerShell,
#     or set up the MSVC env yourself before calling it. On GitHub Actions
#     windows-latest, MSVC is already on PATH.

$ErrorActionPreference = "Stop"

# --- derive version from Cargo.toml --------------------------------------
$cargoToml = Get-Content -Raw -Path "Cargo.toml"
if ($cargoToml -notmatch '(?m)^\s*version\s*=\s*"([^"]+)"') {
    throw "package.ps1: could not read version from Cargo.toml"
}
$version = $Matches[1]
Write-Host "==> version $version"

# --- preflight -----------------------------------------------------------
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "package.ps1: cargo not found on PATH"
}

$nsis = (Get-Command makensis.exe -ErrorAction SilentlyContinue).Source
if (-not $nsis) { $nsis = "${env:ProgramFiles(x86)}\NSIS\makensis.exe" }
if (-not (Test-Path $nsis)) { throw "Install NSIS 3 to build the Windows setup package." }

# --- build ---------------------------------------------------------------
Write-Host "==> cargo build --release"
cargo build --release
if ($LASTEXITCODE -ne 0) {
    throw "package.ps1: cargo build failed (exit=$LASTEXITCODE)"
}

$exePath = "target\release\QuickShot.exe"
if (-not (Test-Path $exePath)) {
    throw "package.ps1: expected $exePath was not produced"
}

# --- stage + zip ---------------------------------------------------------
$staging = "dist\QuickShot-$version-windows-x64"
if (Test-Path "dist") {
    Remove-Item -Recurse -Force "dist"
}
New-Item -ItemType Directory -Force -Path $staging | Out-Null

Copy-Item $exePath "$staging\"
Copy-Item "README.md" "$staging\"

$zip = "dist\QuickShot-$version-windows-x64.zip"
Compress-Archive -Path "$staging\*" -DestinationPath $zip -Force

$installer = Join-Path (Resolve-Path "dist") "QuickShot-$version-windows-x64-Setup.exe"
$payload = (Resolve-Path $exePath).Path
& $nsis "/DVERSION=$version" "/DPAYLOAD=$payload" "/DOUTPUT=$installer" "scripts/installer.nsi"
if ($LASTEXITCODE -ne 0 -or -not (Test-Path $installer)) {
    throw "package.ps1: installer build failed"
}

# --- report --------------------------------------------------------------
Write-Host ""
Write-Host "done:"
Write-Host "  $staging\QuickShot.exe"
$zipItem = Get-Item $zip
$sizeKb = [Math]::Round($zipItem.Length / 1KB, 1)
Write-Host "  $zip  ($sizeKb KB)"
Write-Host "  $installer"
