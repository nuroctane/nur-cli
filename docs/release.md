# Releasing nur-cli

One pipeline, four steps, in this order: **commit -> push -> backup -> install
(last)**. `install` is always last when shipping from nur-cli itself: a release
build replaces the running binary, and doing that before commit/push/backup
would strand the shipped commit.

## The commands

```bash
# From the repo root (or pass -Repo nur-cli from anywhere):
powershell -NoProfile -File %USERPROFILE%\.agents\ship.ps1 -Repo nur-cli -Message "release: vX.Y.Z - <summary>"
```

The script does:

1. `git add -A && git commit -m "<message>"` (refuses a dirty tree without a message)
2. `git push -u origin HEAD`
3. 7z archive -> `D:\BACKUP\CODE Backups\nur-cli\nur-cli_<date>_<sha>_<slug>.7z`
   (`-mmt=4`, build trees and `.nur` excluded)
4. Release build (`cargo build --release`) -> `%USERPROFILE%\.local\bin\nur.exe`,
   then `nur --version` to confirm

Never run the backup step while a build holds the cargo lock, and never let
7z run with its output redirected: a silent 7z looks idle and gets killed.
Run it un-piped so progress streams.

## Version bump

Bump both version strings before the ship commit:

- `Cargo.toml` -> `[package] version`
- `npm/package.json` -> `version`

The release workflow checks the tag against both (`vX.Y.Z`).

## The release workflow

`.github/workflows/release.yml` builds, signs and publishes on a `v*` tag:

| Asset | Built on | Notes |
|---|---|---|
| `nur-windows-x86_64.exe` | windows-latest | signed + sha256 |
| `nur-macos-aarch64` / `nur-macos-x86_64` | macos-latest | |
| `nur-linux-x86_64` | **ubuntu-22.04** | glibc 2.35 floor, not 24.04's 2.39 |
| `nur-linux-x86_64-musl` | ubuntu-22.04 + musl-tools | fully static; runs on Alpine and old distros |

A `smoke` job runs both Linux assets in Debian 12 and Alpine containers
(`--version`) before publish, so a binary that cannot start never ships.

## What a release needs

- `MINISIGN_SECRET_KEY` repo secret (signs every binary and the skill pack)
- tags pushed as `vX.Y.Z` matching `Cargo.toml`
- `nur-skills.zip` is built by the workflow (`scripts/build_skill_pack.py`)
