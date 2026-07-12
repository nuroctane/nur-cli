# Install Meta CLI (unofficial) for the current user (Windows)
# Builds the `muse` binary (Muse Spark agent)
# Usage: powershell -File install.ps1

$ErrorActionPreference = "Stop"
$Repo = $PSScriptRoot
if (-not $Repo) { $Repo = Get-Location }

$cargo = Get-Command cargo -ErrorAction SilentlyContinue
if (-not $cargo) {
    Write-Host "Rust/cargo not found. Installing rustup..." -ForegroundColor Yellow
    winget install --id Rustlang.Rustup -e --accept-package-agreements --accept-source-agreements
    $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
}

$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
Set-Location $Repo
Write-Host "Building Meta CLI / muse (release)..." -ForegroundColor Cyan
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$dest = Join-Path $env:USERPROFILE ".local\bin"
New-Item -ItemType Directory -Force -Path $dest | Out-Null
Copy-Item -Force (Join-Path $Repo "target\release\muse.exe") (Join-Path $dest "muse.exe")

# Ensure ~/.local/bin on user PATH
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$dest*") {
    [Environment]::SetEnvironmentVariable("Path", "$dest;$userPath", "User")
    Write-Host "Added $dest to User PATH (restart terminals)" -ForegroundColor Yellow
}

& (Join-Path $dest "muse.exe") --version
& (Join-Path $dest "muse.exe") install-hook

Write-Host ""
Write-Host "Installed: $dest\muse.exe  (Meta CLI unofficial)" -ForegroundColor Green
Write-Host "Auth: set MODEL_API_KEY or run  muse auth login" -ForegroundColor Green
Write-Host "Orca: orca terminal create --command muse" -ForegroundColor Green
Write-Host "Usage for ADEs: $env:USERPROFILE\.muse\status.json" -ForegroundColor Green
