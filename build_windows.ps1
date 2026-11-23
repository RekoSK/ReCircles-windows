$ErrorActionPreference = "Stop"
Push-Location $PSScriptRoot
try {
$CargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
if ((Test-Path $CargoBin) -and ($env:PATH -notlike "*$CargoBin*")) {
    $env:PATH = "$CargoBin;$env:PATH"
}

$HostTriple = (rustc -vV | Select-String "^host:").ToString().Trim()
if ($HostTriple -notlike "*windows-msvc*") {
    throw "Expected native MSVC Rust toolchain (x86_64-pc-windows-msvc), got: $HostTriple"
}

Write-Host "==> Using native Visual Studio MSVC toolchain ($HostTriple)"

Write-Host "==> [1/5] Building 4 optional submenu DLLs (native MSVC)..."
cargo build --release `
  -p recircles-sub-files `
  -p recircles-sub-apps `
  -p recircles-sub-windows `
  -p recircles-sub-clipboard

Write-Host "==> [2/5] Building recircles.exe and recircles-daemon.exe (native MSVC + Direct2D/DirectComposition)..."
cargo build --release -p recircles-daemon -p recircles-ui

$RelDir = Join-Path $PSScriptRoot "target\release"
$env:RECIRCLES_PAYLOAD_DIR = $RelDir
$env:RECIRCLES_REQUIRE_PAYLOAD = "1"

Write-Host "==> [3/5] Building standalone recircles-setup.exe (native MSVC)..."
cargo build --release -p recircles-installer

Write-Host "==> [4/5] Staging target\dist\..."
$DistDir = Join-Path $PSScriptRoot "target\dist"
$SubDir = Join-Path $DistDir "submenus"
New-Item -ItemType Directory -Force -Path $SubDir | Out-Null
Copy-Item -Force (Join-Path $RelDir "recircles.exe") $DistDir
Copy-Item -Force (Join-Path $RelDir "recircles-daemon.exe") $DistDir
Copy-Item -Force (Join-Path $RelDir "recircles-setup.exe") $DistDir
Get-ChildItem -Path $RelDir -Filter "recircles_*.dll" | ForEach-Object {
    Copy-Item -Force $_.FullName $SubDir
}

Write-Host "==> [5/5] Running workspace tests..."
cargo test --workspace
if ($LASTEXITCODE -ne 0) {
    throw "cargo test --workspace failed with exit code $LASTEXITCODE"
}

Write-Host "Build complete!"
Write-Host "  App:       $DistDir\recircles.exe"
Write-Host "  Daemon:    $DistDir\recircles-daemon.exe"
Write-Host "  Installer: $DistDir\recircles-setup.exe"
} finally {
    Pop-Location
}
