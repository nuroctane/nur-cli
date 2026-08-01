#!/usr/bin/env bash
# One-shot install of NurCLI (unofficial) — builds the `nur` binary.
#
# From a clone:
#   ./install.sh
#
# Remote one-shot:
#   curl -fsSL https://raw.githubusercontent.com/nuroctane/nur-cli/main/install.sh | bash
#
# Secrets are NEVER written into the repo. Keys live only in ~/.nur/auth.json
# or env NUR_API_KEY (META_API_KEY for the Meta provider).

set -euo pipefail

REPO_URL="https://github.com/nuroctane/nur-cli.git"
BRANCH="main"
REPO_DIR="${NUR_CLI_DIR:-$HOME/laboratory/nur-cli}"
SKIP_HOOK="${SKIP_HOOK:-0}"

step() { printf '  → %s\n' "$*"; }
ok()   { printf '  ✓ %s\n' "$*"; }
warn() { printf '  ! %s\n' "$*"; }

echo ""
echo "  NurCLI (unofficial) installer"
echo "  command: nur  ·  Meta Model API agent · not affiliated with Meta"
echo ""

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd || true)"
IN_REPO=0
if [[ -n "${SCRIPT_DIR}" && -f "${SCRIPT_DIR}/Cargo.toml" ]] && grep -q 'name = "nur-cli"' "${SCRIPT_DIR}/Cargo.toml"; then
  REPO_DIR="${SCRIPT_DIR}"
  IN_REPO=1
fi

if [[ "${IN_REPO}" -eq 0 ]]; then
  step "Source: ${REPO_DIR}"
  command -v git >/dev/null || { echo "git is required"; exit 1; }
  mkdir -p "$(dirname "${REPO_DIR}")"
  if [[ -f "${REPO_DIR}/Cargo.toml" ]]; then
    step "Updating existing clone…"
    git -C "${REPO_DIR}" fetch origin "${BRANCH}"
    git -C "${REPO_DIR}" checkout "${BRANCH}"
    git -C "${REPO_DIR}" pull --ff-only origin "${BRANCH}" || true
  else
    step "Cloning ${REPO_URL} …"
    git clone --branch "${BRANCH}" --single-branch "${REPO_URL}" "${REPO_DIR}"
  fi
fi
ok "Repo: ${REPO_DIR}"

export PATH="${HOME}/.cargo/bin:${PATH}"
if ! command -v cargo >/dev/null 2>&1; then
  step "Rust/cargo not found — installing rustup…"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
  # shellcheck disable=SC1091
  source "${HOME}/.cargo/env" 2>/dev/null || export PATH="${HOME}/.cargo/bin:${PATH}"
fi
command -v cargo >/dev/null || { echo "cargo not found after rustup; open a new shell and re-run"; exit 1; }
ok "cargo $(cargo --version)"

# ── prerequisites (auto-install latest when missing; best-effort) ──────────
# node 20+ (plur · ruflo · executor · browser) · bun (omp backend) ·
# uv (graphify) · ripgrep (fast search) · ffmpeg (extract_frames).
step "Checking prerequisites (node · bun · uv · rg · ffmpeg)…"
PKG=""
if command -v brew >/dev/null 2>&1; then PKG="brew"
elif command -v apt-get >/dev/null 2>&1; then PKG="apt"
elif command -v dnf >/dev/null 2>&1; then PKG="dnf"
elif command -v pacman >/dev/null 2>&1; then PKG="pacman"
fi
pkg_install() { # pkg_install <package-name…>
  case "${PKG}" in
    brew)   brew install "$@" ;;
    apt)    sudo apt-get install -y "$@" 2>/dev/null || apt-get install -y "$@" 2>/dev/null ;;
    dnf)    sudo dnf install -y "$@" 2>/dev/null ;;
    pacman) sudo pacman -S --noconfirm "$@" 2>/dev/null ;;
    *)      return 1 ;;
  esac
}
ensure_prereq() { # ensure_prereq <cmd> <pkg-name> <note> [fallback-cmd…]
  local cmd="$1" pkg="$2" note="$3"; shift 3
  if command -v "${cmd}" >/dev/null 2>&1; then
    ok "${cmd} already installed"
    return 0
  fi
  step "Installing ${cmd} — ${note}…"
  if pkg_install "${pkg}"; then
    ok "${cmd} installed"
  elif [[ $# -gt 0 ]] && "$@"; then
    ok "${cmd} installed (official installer)"
  else
    warn "${cmd} could not be auto-installed — needed for: ${note}"
  fi
}
bun_official() { curl -fsSL https://bun.sh/install | bash; export PATH="${HOME}/.bun/bin:${PATH}"; }
uv_official()  { curl -LsSf https://astral.sh/uv/install.sh | sh; export PATH="${HOME}/.local/bin:${PATH}"; }
node_pkg="node"; [[ "${PKG}" == "apt" || "${PKG}" == "dnf" ]] && node_pkg="nodejs"
ensure_prereq node   "${node_pkg}" "plur · ruflo · executor · browser"
ensure_prereq bun    "oven-sh/bun/bun" "omp coding-agent backend" bun_official
ensure_prereq uv     "uv"          "graphify" uv_official
ensure_prereq rg     "ripgrep"     "fast grep / glob"
ensure_prereq ffmpeg "ffmpeg"      "extract_frames / design-from-video"
export PATH="${HOME}/.bun/bin:${HOME}/.local/bin:${PATH}"

step "Building release (first time can take a few minutes)…"
( cd "${REPO_DIR}" && cargo build --release )
BUILT="${REPO_DIR}/target/release/nur"
[[ -f "${BUILT}" ]] || { echo "missing release binary"; exit 1; }

DEST_DIR="${HOME}/.local/bin"
mkdir -p "${DEST_DIR}"
# Integrity: SHA-256 of the release binary (written next to install + verified after copy).
if command -v sha256sum >/dev/null 2>&1; then
  BUILT_HASH="$(sha256sum "${BUILT}" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
  BUILT_HASH="$(shasum -a 256 "${BUILT}" | awk '{print $1}')"
else
  BUILT_HASH=""
  warn "sha256sum/shasum not found — skipping binary integrity hash"
fi
cp -f "${BUILT}" "${DEST_DIR}/nur"
chmod +x "${DEST_DIR}/nur"
# Enforce the single-command install contract on upgrades too.
rm -f "${DEST_DIR}/muse" "${DEST_DIR}/muse.exe" \
  "${DEST_DIR}/muse-opencode.cmd" "${DEST_DIR}/muse-opencode.ps1" \
  "${DEST_DIR}/muse.sha256" "${DEST_DIR}/meta" "${DEST_DIR}/meta.exe" \
  "${DEST_DIR}/meta.sha256"
if [[ -n "${BUILT_HASH}" ]]; then
  INSTALLED_HASH="$( (sha256sum "${DEST_DIR}/nur" 2>/dev/null || shasum -a 256 "${DEST_DIR}/nur") | awk '{print $1}' )"
  if [[ "${INSTALLED_HASH}" != "${BUILT_HASH}" ]]; then
    echo "Integrity check failed: installed nur hash does not match build" >&2
    exit 1
  fi
  echo "${BUILT_HASH}  nur" > "${DEST_DIR}/nur.sha256"
  ok "SHA-256 ${BUILT_HASH}"
fi
export PATH="${DEST_DIR}:${PATH}"

for rc in "${HOME}/.zprofile" "${HOME}/.zshrc" "${HOME}/.bash_profile" "${HOME}/.bashrc" "${HOME}/.profile"; do
  if [[ -f "${rc}" ]] && ! grep -q '\.local/bin' "${rc}" 2>/dev/null; then
    echo "export PATH=\"\$HOME/.local/bin:\$PATH\"" >> "${rc}"
    ok "Appended ~/.local/bin to ${rc}"
    break
  fi
done

ok "Installed ${DEST_DIR}/nur ($("${DEST_DIR}/nur" --version))"

# ── Ecosystem: Graphify · PLUR · Ruflo · omp · browser (blocking) ─────────
step "Provisioning agent ecosystem (graphify · plur · ruflo · omp · browser)…"
"${DEST_DIR}/nur" ecosystem ensure --force || warn "Ecosystem ensure incomplete — re-run: nur install"
ok "Ecosystem provisioned"

# ── Browser tool: stage extension + target the default browser ────────────
# Usable immediately; the one-time "load unpacked" click is a Chromium
# security step we surface but can't automate.
"${DEST_DIR}/nur" browser setup 2>/dev/null || warn "Browser setup deferred — run later: nur browser setup"

if [[ "${SKIP_HOOK}" != "1" ]]; then
  "${DEST_DIR}/nur" install-hook >/dev/null 2>&1 && ok "Orca hook installed (if applicable)" || true
fi

KEY="${NUR_API_KEY:-${META_API_KEY:-}}"
if [[ -n "${KEY}" ]]; then
  step "API key found in environment — saving to ~/.nur/auth.json (local only)…"
  "${DEST_DIR}/nur" auth login --key "${KEY}" >/dev/null
  ok "Auth stored under ~/.nur/ (never committed to git)"
else
  warn "No API key in env yet. After install:  nur auth login"
  warn "Get a key: https://dev.meta.ai/"
fi

echo ""
echo "  Done."
echo "  Run:   nur"
echo "  Auth:  nur auth login     (key stays in ~/.nur only)"
echo "  Stack: graphify + plur + ruflo installed during this run"
echo "  Orca:  orca terminal create --command nur"
echo "  Docs:  https://github.com/nuroctane/nur-cli"
echo ""
