//! Route plain `cargo build|check|test|run` through blaze, the incremental
//! Rust compiler (<https://github.com/indexable-inc/blaze>), when the user
//! opted in with `[tools] rust_builder`.
//!
//! Like sgrep routing, a command is only ever replaced by a better answer:
//! - only the plain form routes (no `+toolchain`, no shell operators, none of
//!   the flags blaze refuses); everything else runs on cargo untouched;
//! - a blaze refusal ("does not build ... yet") runs the original cargo
//!   command and says so; blaze's compile errors are real results;
//! - a missing blaze binary leaves routing inert;
//! - `auto` routes only where blaze ships (Linux x86-64 and aarch64, macOS
//!   arm64). blaze's borrow checker is partial, so every blaze result reminds
//!   the agent to run `cargo check` once before calling the work done.
//!
//! `NUR_BLAZE_BIN` overrides the binary (a command line, so tests can use a
//! script).

/// Subcommands blaze replaces.
const ROUTED: &[&str] = &["build", "check", "test", "run"];
/// Flags blaze refuses rather than ignores, plus `--release` (not its job).
const REFUSED_FLAGS: &[&str] = &[
    "--release",
    "-r",
    "--profile",
    "--all-targets",
    "--bins",
    "--examples",
    "--tests",
    "--benches",
    "--target-dir",
    "-Z",
];

/// The reminder every blaze result carries.
pub const BORROW_CHECK_NOTE: &str = "note: built with blaze, which does not run the full borrow \
                                     checker; run `cargo check` once before reporting the work done.";

/// Whether blaze runs on this platform.
fn supported_platform() -> bool {
    cfg!(any(
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        ),
        all(target_os = "macos", target_arch = "aarch64")
    ))
}

/// The blaze command line to run instead of `command`, or `None` to run it
/// as written. `mode` is `[tools] rust_builder`.
pub fn route(command: &str, mode: &str) -> Option<String> {
    let routing = match mode {
        "blaze" => true,
        "auto" => supported_platform(),
        _ => false,
    };
    if !routing {
        return None;
    }
    let rest = plain_cargo(command)?;
    // Looked up once: a missing blaze would otherwise cost a PATH probe (and
    // the first time an npm launch) on every cargo command.
    static FOUND: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    let blaze = std::env::var("NUR_BLAZE_BIN")
        .ok()
        .filter(|b| !b.trim().is_empty())
        .or_else(|| {
            FOUND
                .get_or_init(|| crate::ecosystem::find_bin("blaze"))
                .clone()
        })?;
    let blaze = if blaze.contains(' ') && std::path::Path::new(&blaze).exists() {
        format!("\"{blaze}\"")
    } else {
        blaze
    };
    Some(format!("{blaze} {rest}"))
}

/// `build|check|test|run [args]` when `command` is exactly a plain cargo
/// invocation blaze can take.
fn plain_cargo(command: &str) -> Option<&str> {
    let trimmed = command.trim();
    if trimmed.contains(['|', '&', ';', '>', '<', '`', '$', '\n']) {
        return None;
    }
    let rest = trimmed.strip_prefix("cargo ")?.trim_start();
    let sub = rest.split_whitespace().next()?;
    if !ROUTED.contains(&sub) {
        return None;
    }
    let args: Vec<&str> = rest.split_whitespace().skip(1).collect();
    let refused = args.iter().any(|a| {
        REFUSED_FLAGS
            .iter()
            .any(|flag| *a == *flag || a.starts_with(&format!("{flag}=")))
            || a.starts_with("-Z")
    });
    let targets = args.iter().filter(|a| a.starts_with("--target")).count();
    (!refused && targets <= 1).then_some(rest)
}

/// blaze declined the command (an unsupported flag or target), as opposed to
/// a compile error. It says so before doing anything, so only the opening
/// lines count: a program or test that printed those words ran already and
/// must not run a second time on cargo.
pub fn refused(output: &str) -> bool {
    output
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(3)
        .any(|l| l.contains("does not build") && l.contains("yet"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Failure modes: a command blaze cannot take is routed (and fails);
    // a compound shell line is half-routed; a release build or toolchain pin
    // silently changes compilers; routing happens where blaze does not run.

    #[test]
    fn only_plain_supported_cargo_commands_route() {
        assert_eq!(plain_cargo("cargo build"), Some("build"));
        assert_eq!(
            plain_cargo("cargo test -p core it_works"),
            Some("test -p core it_works")
        );
        for command in [
            "cargo build --release",
            "cargo +nightly build",
            "cargo test --all-targets",
            "cargo check && cargo test",
            "cargo test 2>&1 | tail",
            "cargo clippy",
            "cargo build -Zunstable-options",
            "cargo build --target a --target b",
        ] {
            assert_eq!(plain_cargo(command), None, "{command}");
        }
    }

    #[test]
    fn cargo_mode_never_routes_and_refusals_are_recognized() {
        assert_eq!(route("cargo build", "cargo"), None);
        assert!(refused("error: blaze does not build --all-targets yet"));
        assert!(!refused("error[E0382]: borrow of moved value"));
    }
}
