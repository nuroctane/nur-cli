#!/usr/bin/env bash
# Install Meta CLI (unofficial) — builds the `muse` binary
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found — install Rust from https://rustup.rs"
  exit 1
fi

cargo build --release
DEST="${HOME}/.local/bin"
mkdir -p "$DEST"
cp -f target/release/muse "$DEST/muse"
chmod +x "$DEST/muse"

case ":$PATH:" in
  *":$DEST:"*) ;;
  *) echo "Add $DEST to PATH (e.g. export PATH=\"$DEST:\$PATH\")" ;;
esac

"$DEST/muse" --version
"$DEST/muse" install-hook || true
echo "Installed $DEST/muse (Meta CLI unofficial)"
echo "Auth: export MODEL_API_KEY=...  or  muse auth login"
echo "ADE status: ~/.muse/status.json"
