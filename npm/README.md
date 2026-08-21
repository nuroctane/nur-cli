# nur-cli (npm shim)

One command installs NurCLI on Windows, macOS, or Linux:

```bash
npx nur-cli
```

or, to keep it on PATH permanently:

```bash
npm i -g nur-cli
```

## What it does

1. Downloads the **prebuilt native binary** from
   [GitHub Releases](https://github.com/nuroctane/nur-cli/releases/latest)
   (`nur-windows-x86_64.exe`, `nur-macos-aarch64`, `nur-macos-x86_64`,
   `nur-linux-x86_64`). No rustup, no clone, no `cargo build`.
2. Installs it to `~/.local/bin` (`%USERPROFILE%\.local\bin\nur.exe` on Windows).
3. Runs `nur install` - the same one-stop setup as the release EXE: user PATH,
   prerequisites (best-effort), ecosystem packs (Graphify / PLUR / Ruflo /
   Executor / omp / browser / skills), and first-run auth hints.

The npm package is a zero-dependency downloader (~4 KB). All real logic lives in
the Rust binary, so npm users get exactly the same product as the one-liner.

## Requirements

- Node.js 18+ (only for the download step; `nur` itself is a native binary)
- Internet access to `github.com`

## Publish

```bash
cd npm
npm publish
```

Bump `version` in `npm/package.json` to match the release tag before publishing.
`FALLBACK_VERSION` inside `bin.js` should match too (it is only used when the
`latest/download` redirect is unreachable).

## Notes

- `postinstall` runs `node bin.js --ensure`, so a plain `npm i -g nur-cli`
  performs the full install without a second command. It skips re-downloading
  when the binary is already present.
- Re-running `npx nur-cli` always fetches the latest release binary, then
  refreshes the stack via `nur install`.
- `nur update` continues to self-update the native binary directly from GitHub
  Releases; the npm package never needs to be involved again after install.

---

## License

**GNU General Public License v3.0 (or later)** — see [LICENSE](./LICENSE).

Meta CLI is free software: you may redistribute it and/or modify it under the
terms of the GPL as published by the Free Software Foundation, either version 3
of the License, or (at your option) any later version. It is distributed in the
hope that it will be useful, but **without any warranty**; without even the
implied warranty of merchantability or fitness for a particular purpose.
