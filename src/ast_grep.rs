//! ast-grep - structural code search and rewrite on tree-sitter
//! (<https://ast-grep.github.io>, MIT). The engine behind the `ast_grep`
//! tool: patterns are real code with `$VAR` / `$$$` wildcards, so a search
//! matches syntax, not text.
//!
//! This module owns discovery and installation state. The tool lives in
//! `tools::ast_grep`. nur only ever runs the `ast-grep` binary, never its
//! `sg` alias (on Linux `sg` is `setgroups`).

use std::fs;
use std::path::{Path, PathBuf};

/// npm package that ships the prebuilt binaries (pinned in the ecosystem lock).
pub const NPM_PACKAGE: &str = "@ast-grep/cli";
/// Human install line, shown when the binary is missing.
pub const INSTALL_HINT: &str = "npm i -g @ast-grep/cli  (or: cargo install ast-grep --locked, brew install ast-grep, nur ecosystem ensure)";
/// Environment override for the binary location. Empty (or a missing file)
/// disables the tool, which the E2E suite uses to pin the missing path.
pub const BIN_ENV: &str = "NUR_AST_GREP_BIN";

/// Resolve the `ast-grep` binary: `NUR_AST_GREP_BIN`, then the native
/// executable behind an npm global install, then the usual fixed dirs and PATH.
pub fn ast_grep_bin() -> Option<String> {
    if let Ok(v) = std::env::var(BIN_ENV) {
        let v = v.trim();
        if v.is_empty() {
            return None;
        }
        return Path::new(v).is_file().then(|| v.to_string());
    }
    // Only a hit is cached: a binary that `nur ecosystem ensure` installs
    // mid-session is picked up on the next call.
    static BIN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    if let Some(found) = BIN.get() {
        return Some(found.clone());
    }
    let found = resolve()?;
    Some(BIN.get_or_init(|| found).clone())
}

/// Install the pinned npm package (prebuilt binaries, seconds), or
/// `cargo install ast-grep --locked` where npm is missing (a compile).
/// Bounded and marked, so a failing install is retried weekly at most.
pub fn install() -> Option<String> {
    mark_install_attempted();
    if let Some(npm) = crate::ecosystem::find_bin("npm") {
        let spec = crate::ecosystem::lock::npm_spec(NPM_PACKAGE);
        let _guard = crate::ecosystem::npm_lock();
        let _ = crate::ecosystem::run_capture(&npm, &["install", "-g", &spec], None, 300_000);
    } else if crate::ecosystem::which("cargo") || crate::ecosystem::which("cargo.exe") {
        let _ = crate::ecosystem::run_quiet(
            "cargo",
            &["install", "ast-grep", "--locked"],
            None,
            1_800_000,
        );
    }
    ast_grep_bin()
}

fn resolve() -> Option<String> {
    let found = crate::ecosystem::find_bin("ast-grep")?;
    Some(native_behind_shim(Path::new(&found)).unwrap_or(found))
}

/// npm's Windows `ast-grep.cmd` shim starts Node, which starts the binary.
/// Batch files re-parse their arguments (`%`, `"`, `^` in a pattern), so the
/// tool runs the package's own executable next to the shim instead.
fn native_behind_shim(shim: &Path) -> Option<String> {
    let ext = shim
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !matches!(ext.as_str(), "cmd" | "ps1" | "") {
        return None;
    }
    let pkg = shim
        .parent()?
        .join("node_modules")
        .join("@ast-grep")
        .join("cli");
    let names: &[&str] = if cfg!(windows) {
        &["ast-grep.exe"]
    } else {
        &["ast-grep"]
    };
    names
        .iter()
        .map(|n| pkg.join(n))
        .find(|p| p.is_file() && (cfg!(windows) || ext.is_empty()))
        .map(|p: PathBuf| p.to_string_lossy().into_owned())
}

fn marker() -> PathBuf {
    crate::config::nur_home()
        .join("cache")
        .join("ast-grep-install-attempted")
}

/// One-shot marker: a failing install is retried at most once per 7 days, so
/// a broken npm or toolchain cannot stall every ensure.
pub fn install_attempted() -> bool {
    match fs::metadata(marker()) {
        Ok(m) => m
            .modified()
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_none_or(|age| age.as_secs() < 7 * 24 * 3600),
        Err(_) => false,
    }
}

pub fn mark_install_attempted() {
    let m = marker();
    if let Some(dir) = m.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(m, "1");
}

#[cfg(test)]
mod tests {
    use super::*;

    // Failure mode: the Windows npm shim was run directly, so a pattern with
    // `%` or `"` was re-parsed by cmd.exe and the search matched nothing.
    #[test]
    fn npm_shim_resolves_to_the_native_binary_beside_it() {
        let dir = std::env::temp_dir().join(format!("nur-astgrep-shim-{}", std::process::id()));
        let pkg = dir.join("node_modules").join("@ast-grep").join("cli");
        fs::create_dir_all(&pkg).unwrap();
        let native = pkg.join(if cfg!(windows) {
            "ast-grep.exe"
        } else {
            "ast-grep"
        });
        fs::write(&native, b"bin").unwrap();
        let shim = dir.join(if cfg!(windows) {
            "ast-grep.cmd"
        } else {
            "ast-grep"
        });
        fs::write(&shim, b"@echo off").unwrap();
        let got = native_behind_shim(&shim);
        let other = native_behind_shim(&dir.join("ast-grep.exe"));
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(got.as_deref(), Some(native.to_string_lossy().as_ref()));
        if cfg!(windows) {
            assert_eq!(other, None, "a real .exe is used as found");
        }
    }
}
