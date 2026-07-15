#Requires -Version 5.1
<#
.SYNOPSIS
  One-shot install of NurCLI (unofficial) — builds the `nur` binary ().

.DESCRIPTION
  Works two ways:
    1) From a clone:  .\install.ps1
    2) Remote one-shot (no clone yet):
         irm https://raw.githubusercontent.com/nuroctane/nur-cli/main/install.ps1 | iex

  Steps: ensure Rust → clone if needed → cargo build --release →
  install nur to %USERPROFILE%\.local\bin → PATH → Orca hook →
  optional auth if NUR_API_KEY (or legacy META_/MODEL_/MUSE_) is set.

  Secrets are NEVER written into the repo. Keys live only in:
    %USERPROFILE%\.nur\auth.json   or   env NUR_API_KEY (legacy META_/MODEL_/MUSE_)

.PARAMETER SkipHook
  Skip Orca agent-hook install.

.PARAMETER RepoDir
  Where to clone/build (default: %USERPROFILE%\laboratory\nur-cli).
#>
param(
    [switch]$SkipHook,
    [string]$RepoDir = ""
)

$ErrorActionPreference = "Stop"
$RepoUrl = "https://github.com/nuroctane/nur-cli.git"
$Branch = "main"

function Write-Step($msg) { Write-Host "  → $msg" -ForegroundColor Cyan }
function Write-Ok($msg)   { Write-Host "  ✓ $msg" -ForegroundColor Green }
function Write-Warn($msg) { Write-Host "  ! $msg" -ForegroundColor Yellow }

Write-Host ""
Write-Host "  NurCLI (unofficial) installer" -ForegroundColor Blue
Write-Host "  Meta Model API agent · not affiliated with Meta" -ForegroundColor DarkGray
Write-Host ""

# ── locate or clone source ────────────────────────────────────────────────
$scriptRoot = $PSScriptRoot
if (-not $scriptRoot -and $MyInvocation.MyCommand.Path) {
    $scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
}

$inRepo = $false
if ($scriptRoot -and (Test-Path (Join-Path $scriptRoot "Cargo.toml"))) {
    $toml = Get-Content (Join-Path $scriptRoot "Cargo.toml") -Raw
    if ($toml -match 'name\s*=\s*"nur-cli"') {
        $RepoDir = $scriptRoot
        $inRepo = $true
    }
}

if (-not $RepoDir) {
    $RepoDir = Join-Path $env:USERPROFILE "laboratory\nur-cli"
}

if (-not $inRepo) {
    Write-Step "Source: $RepoDir"
    if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
        throw "git is required. Install Git for Windows: https://git-scm.com/download/win"
    }
    $parent = Split-Path -Parent $RepoDir
    if (-not (Test-Path $parent)) {
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
    }
    if (Test-Path (Join-Path $RepoDir "Cargo.toml")) {
        Write-Step "Updating existing clone (git pull)…"
        Push-Location $RepoDir
        try {
            git fetch origin $Branch 2>$null | Out-Null
            git checkout $Branch 2>$null | Out-Null
            git pull --ff-only origin $Branch 2>$null | Out-Null
        } finally { Pop-Location }
    } else {
        if (Test-Path $RepoDir) {
            throw "Directory exists but is not a nur-cli checkout: $RepoDir"
        }
        Write-Step "Cloning $RepoUrl …"
        git clone --branch $Branch --single-branch $RepoUrl $RepoDir
        if ($LASTEXITCODE -ne 0) { throw "git clone failed" }
    }
}

Write-Ok "Repo: $RepoDir"

# ── Rust toolchain ────────────────────────────────────────────────────────
$cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
$env:Path = "$cargoBin;$env:Path"

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Step "Rust/cargo not found — installing rustup (stable)…"
    $rustup = Join-Path $env:TEMP "rustup-init-nur-cli.exe"
    Invoke-WebRequest -Uri "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe" -OutFile $rustup
    & $rustup -y --default-toolchain stable
    if ($LASTEXITCODE -ne 0) {
        Write-Warn "rustup-init failed; trying winget…"
        winget install --id Rustlang.Rustup -e --accept-package-agreements --accept-source-agreements
    }
    $env:Path = "$cargoBin;$env:Path"
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        throw "cargo still not on PATH. Open a new terminal and re-run, or install from https://rustup.rs"
    }
}
Write-Ok "cargo $((cargo --version))"

# ── prerequisites (auto-install latest when missing; best-effort) ─────────
# node 20+ (plur · ruflo · executor · browser) · bun (omp backend) ·
# uv (graphify) · ripgrep (fast search) · ffmpeg (extract_frames).
Write-Step "Checking prerequisites (node · bun · uv · rg · ffmpeg)…"
$wingetCmd = Get-Command winget -ErrorAction SilentlyContinue
function Install-Prereq([string]$cmd, [string]$wingetId, [scriptblock]$fallback, [string]$note) {
    if (Get-Command $cmd -ErrorAction SilentlyContinue) {
        Write-Ok "$cmd already installed"
        return
    }
    Write-Step "Installing $cmd — $note…"
    $done = $false
    if ($wingetCmd -and $wingetId) {
        winget install --id $wingetId -e --silent --accept-package-agreements --accept-source-agreements 2>$null | Out-Null
        if ($LASTEXITCODE -eq 0) { $done = $true }
    }
    if (-not $done -and $fallback) {
        try { & $fallback; $done = $true } catch { }
    }
    if ($done) {
        Write-Ok "$cmd installed (open a new terminal if it's not on PATH yet)"
    } else {
        Write-Warn "$cmd could not be auto-installed — needed for: $note"
    }
}
Install-Prereq "node"   "OpenJS.NodeJS.LTS"      $null                                     "plur · ruflo · executor · browser"
Install-Prereq "bun"    "Oven-sh.Bun"            { irm https://bun.sh/install.ps1 | iex }  "omp coding-agent backend"
Install-Prereq "uv"     "astral-sh.uv"           { irm https://astral.sh/uv/install.ps1 | iex } "graphify"
Install-Prereq "rg"     "BurntSushi.ripgrep.MSVC" $null                                    "fast grep / glob"
Install-Prereq "ffmpeg" "Gyan.FFmpeg"            $null                                     "extract_frames / design-from-video"
# Make fresh installs visible to this session (winget links, node, bun, uv).
$env:Path = "$env:USERPROFILE\.local\bin;$env:USERPROFILE\.bun\bin;" +
            "$env:LOCALAPPDATA\Microsoft\WinGet\Links;$env:ProgramFiles\nodejs;" + $env:Path

# ── build ─────────────────────────────────────────────────────────────────
Push-Location $RepoDir
try {
    Write-Step "Building release (first time can take a few minutes)…"
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build --release failed" }
} finally { Pop-Location }

$built = Join-Path $RepoDir "target\release\nur.exe"
if (-not (Test-Path $built)) { throw "missing release binary: $built" }

# ── install binary ────────────────────────────────────────────────────────
$destDir = Join-Path $env:USERPROFILE ".local\bin"
New-Item -ItemType Directory -Force -Path $destDir | Out-Null
# Primary (and only) command is `nur`.
function Install-BinarySafe([string]$Source, [string]$Target) {
    try {
        Copy-Item -Force $Source $Target -ErrorAction Stop
        return $true
    } catch {
        # Binary locked by a running TUI — swap via rename so `nur` still updates.
        $bak = "$Target.old"
        try {
            if (Test-Path $bak) { Remove-Item -Force $bak -ErrorAction SilentlyContinue }
            if (Test-Path $Target) { Rename-Item -Force $Target (Split-Path $bak -Leaf) -ErrorAction Stop }
            Copy-Item -Force $Source $Target -ErrorAction Stop
            Remove-Item -Force $bak -ErrorAction SilentlyContinue
            return $true
        } catch {
            Write-Warn "Could not replace $Target (is nur still running?). Close it and re-run install."
            return $false
        }
    }
}
$dest = Join-Path $destDir "nur.exe"
# Integrity: SHA-256 of the release binary (written next to install + verified after copy).
$builtHash = (Get-FileHash -Algorithm SHA256 -Path $built).Hash.ToLowerInvariant()
if (-not (Install-BinarySafe $built $dest)) {
    throw "Failed to install primary binary: $dest — quit any running nur session and re-run."
}
# Drop legacy muse.exe alias if present (rebrand — no muse starter).
$legacyMuse = Join-Path $destDir "muse.exe"
if (Test-Path $legacyMuse) {
    try { Remove-Item -Force $legacyMuse -ErrorAction SilentlyContinue } catch {}
}
$legacyMeta = Join-Path $destDir "meta.exe"
if (Test-Path $legacyMeta) {
    try { Remove-Item -Force $legacyMeta -ErrorAction SilentlyContinue } catch {}
}
$installedHash = (Get-FileHash -Algorithm SHA256 -Path $dest).Hash.ToLowerInvariant()
if ($installedHash -ne $builtHash) {
    throw "Integrity check failed: installed nur.exe hash does not match build ($builtHash vs $installedHash)"
}
Set-Content -Path (Join-Path $destDir "nur.sha256") -Value "$builtHash  nur.exe" -Encoding ascii
Write-Ok "SHA-256 $builtHash"
$env:Path = "$destDir;$env:Path"

# Persist User PATH
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if (-not $userPath) { $userPath = "" }
if ($userPath -notlike "*$destDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$destDir;$userPath", "User")
    Write-Ok "Added $destDir to User PATH (new terminals pick it up automatically)"
} else {
    Write-Ok "PATH already includes $destDir"
}

$ver = & $dest --version
Write-Ok "Installed $dest ($ver)"

# ── Ecosystem: Graphify · PLUR · Ruflo (blocking — same as release EXE) ─
# Node is required for plur/ruflo; uv for graphify. Best-effort but runs now.
Write-Step "Provisioning agent ecosystem (graphify · plur · ruflo · omp · browser)…"
try {
    & $dest ecosystem ensure --force 2>&1 | ForEach-Object { Write-Host "    $_" -ForegroundColor DarkGray }
    Write-Ok "Ecosystem provisioned"
} catch {
    Write-Warn "Ecosystem ensure incomplete: $($_.Exception.Message) — re-run: nur install"
}

# ── Browser tool: stage extension + target the default browser ────────────
# The `browser` tool drives the user's real browser (Arc / Chrome / Edge / …).
# Stage the extension now so it's usable immediately; the one-time "load
# unpacked" click is a Chromium security step we surface but can't automate.
try {
    & $dest browser setup 2>&1 | ForEach-Object { Write-Host "    $_" -ForegroundColor DarkGray }
} catch {
    Write-Warn "Browser setup deferred — run later: nur browser setup"
}

# ── Orca hook (best-effort) ───────────────────────────────────────────────
if (-not $SkipHook) {
    try {
        & $dest install-hook 2>$null | Out-Null
        Write-Ok "Orca hook installed (if Orca is present)"
    } catch {
        Write-Warn "Orca hook skipped ($($_.Exception.Message))"
    }
}

# ── auth: never print the key ─────────────────────────────────────────────
$key = $env:NUR_API_KEY
if (-not $key) { $key = $env:META_API_KEY }
if (-not $key) { $key = $env:MODEL_API_KEY }
if (-not $key) { $key = $env:MUSE_API_KEY }
if (-not $key) {
    $key = [Environment]::GetEnvironmentVariable("NUR_API_KEY", "User")
}
if (-not $key) {
    $key = [Environment]::GetEnvironmentVariable("META_API_KEY", "User")
}
if (-not $key) {
    $key = [Environment]::GetEnvironmentVariable("MODEL_API_KEY", "User")
}
if (-not $key) {
    $key = [Environment]::GetEnvironmentVariable("MUSE_API_KEY", "User")
}

if ($key -and $key.Trim().Length -gt 0) {
    Write-Step "API key found in environment — saving to ~/.nur/auth.json (local only)…"
    # Prefer NUR_API_KEY so auth login matches current nomenclature.
    $env:NUR_API_KEY = $key.Trim()
    & $dest auth login --key $env:NUR_API_KEY 2>$null | Out-Null
    Write-Ok "Auth stored under $env:USERPROFILE\.nur\ (never committed to git)"
} else {
    Write-Warn "No API key in env yet. After install:"
    Write-Host "      nur auth login" -ForegroundColor DarkGray
    Write-Host "    or set User env NUR_API_KEY (or META_API_KEY for Meta Model API)" -ForegroundColor DarkGray
}

Write-Host ""
Write-Host "  Done." -ForegroundColor Green
Write-Host "  Run:   nur" -ForegroundColor White
Write-Host "  Auth:  nur auth login     (key stays in ~/.nur only)" -ForegroundColor DarkGray
Write-Host "  Stack: graphify + plur + ruflo installed during this run" -ForegroundColor DarkGray
Write-Host "  Orca:  orca terminal create --command nur" -ForegroundColor DarkGray
Write-Host "  Docs:  https://github.com/nuroctane/nur-cli" -ForegroundColor DarkGray
Write-Host ""
