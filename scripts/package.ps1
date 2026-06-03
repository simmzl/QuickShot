# QuickShot Windows packaging script — produces
# dist\QuickShot-<VERSION>-windows-x64.zip (contains QuickShot.exe + README.md).
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

# --- report --------------------------------------------------------------
Write-Host ""
Write-Host "done:"
Write-Host "  $staging\QuickShot.exe"
$zipItem = Get-Item $zip
$sizeKb = [Math]::Round($zipItem.Length / 1KB, 1)
Write-Host "  $zip  ($sizeKb KB)"
