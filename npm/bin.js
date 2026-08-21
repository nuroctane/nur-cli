#!/usr/bin/env node
/**
 * nur-cli npm shim - `npx nur-cli` in one command.
 *
 * Downloads the prebuilt native `nur` binary from GitHub Releases (no rustup,
 * no clone, no cargo build), drops it in ~/.local/bin, and hands off to
 * `nur install` so the full ecosystem stack provisions exactly as the
 * one-liner / release-EXE paths do.
 *
 * Zero runtime dependencies: Node 18+ builtins only (https, fs, os, child_process).
 */

"use strict";

const https = require("https");
const http = require("http");
const fs = require("fs");
const path = require("path");
const os = require("os");
const { spawnSync } = require("child_process");

const REPO = "nuroctane/nur-cli";
// Falls back to this pinned version when `latest/download` is unreachable.
const FALLBACK_VERSION = "0.27.20";

function fail(msg) {
  process.stderr.write(`nur-cli: ${msg}\n`);
  process.exit(1);
}

function platformAsset() {
  const plat = process.platform;
  const arch = process.arch;
  if (plat === "win32") {
    if (arch !== "x64") fail(`unsupported Windows arch: ${arch} (x64 only today)`);
    // Newer releases use nur-windows-x86_64.exe; older ones shipped nur.exe.
    return { names: ["nur-windows-x86_64.exe", "nur.exe"], execBit: false };
  }
  if (plat === "darwin") {
    const a = arch === "arm64" ? "aarch64" : "x86_64";
    return { names: [`nur-macos-${a}`, `nur-darwin-${a}`], execBit: true };
  }
  if (plat === "linux") {
    if (arch !== "x64") fail(`unsupported Linux arch: ${arch} (x64 only today)`);
    return { names: ["nur-linux-x86_64"], execBit: true };
  }
  fail(`unsupported platform: ${plat}`);
}

/** Follow redirects; resolve with the body Buffer, reject on non-200. */
function fetchBuffer(url, redirects) {
  const maxRedirects = redirects == null ? 6 : redirects;
  return new Promise((resolve, reject) => {
    const mod = url.startsWith("http://") ? http : https;
    const req = mod.get(
      url,
      { headers: { "user-agent": "nur-cli-npm-shim" } },
      (res) => {
        const status = res.statusCode || 0;
        if (status >= 300 && status < 400 && res.headers.location) {
          res.resume();
          if (maxRedirects <= 0) return reject(new Error("too many redirects"));
          const next = new URL(res.headers.location, url).toString();
          return resolve(fetchBuffer(next, maxRedirects - 1));
        }
        if (status !== 200) {
          res.resume();
          return reject(new Error(`HTTP ${status} for ${url}`));
        }
        const chunks = [];
        res.on("data", (c) => chunks.push(c));
        res.on("end", () => resolve(Buffer.concat(chunks)));
        res.on("error", reject);
      }
    );
    req.on("error", reject);
    req.setTimeout(120000, () => {
      req.destroy(new Error("download timed out after 120s"));
    });
  });
}

function installDir() {
  // Keep parity with the Rust installer default (~/.local/bin).
  return process.env.NUR_INSTALL_DIR || path.join(os.homedir(), ".local", "bin");
}

function installedBinaryPath() {
  const dir = installDir();
  return path.join(dir, process.platform === "win32" ? "nur.exe" : "nur");
}

async function downloadAndInstall() {
  const asset = platformAsset();
  const dir = installDir();
  fs.mkdirSync(dir, { recursive: true });

  // Try every (asset, version) combination: latest first, then the pinned
  // fallback version. Covers legacy release layouts too.
  const versions = ["latest/download", `download/v${FALLBACK_VERSION}`];
  const urls = [];
  for (const v of versions) {
    for (const name of asset.names) {
      urls.push(`https://github.com/${REPO}/releases/${v}/${name}`);
    }
  }

  let buf = null;
  let lastErr = null;
  let picked = null;
  for (const url of urls) {
    try {
      process.stdout.write(`nur-cli: downloading ${url.split("/").pop()} ...\n`);
      buf = await fetchBuffer(url);
      picked = url;
      break;
    } catch (e) {
      lastErr = e;
    }
  }
  if (!buf) fail(`download failed (${lastErr}). Check https://github.com/${REPO}/releases`);

  if (buf.length < 1000000) {
    fail(`downloaded asset too small (${buf.length} bytes) - aborting`);
  }

  const dest = installedBinaryPath();
  fs.writeFileSync(dest, buf);
  if (asset.execBit) fs.chmodSync(dest, 0o755);
  process.stdout.write(`nur-cli: installed ${dest}\n`);
  return dest;
}

function runNurInstall(bin) {
  // Full one-stop install: PATH wiring, prereqs, ecosystem packs, browser.
  // The binary owns all of it - same as double-clicking a release EXE.
  const r = spawnSync(bin, ["install"], { stdio: "inherit" });
  if (r.error) {
    process.stdout.write(
      `nur-cli: could not run "${bin} install" (${r.error.message}).\n` +
        `Binary is installed - run it manually once to finish setup.\n`
    );
  }
}

async function main() {
  const args = process.argv.slice(2);

  // --ensure: postinstall hook mode. Skip when the binary already exists so
  // `npm i -g nur-cli` upgrades do not re-download on every version bump.
  if (args.includes("--ensure")) {
    if (fs.existsSync(installedBinaryPath())) {
      process.stdout.write("nur-cli: native binary already installed\n");
      return;
    }
    const bin = await downloadAndInstall();
    runNurInstall(bin);
    return;
  }

  // Default: always fetch latest, then hand off to `nur install`.
  const bin = await downloadAndInstall();
  runNurInstall(bin);
}

main().catch((e) => fail(e && e.stack ? e.stack : String(e)));
