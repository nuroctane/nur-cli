mod ade;
mod agent;
mod api;
mod auth;
mod bench;
mod bootstrap;
mod cli;
mod config;
mod ecosystem;
mod error;
mod gateway;
mod local;
mod oauth;
mod open_uri;
mod plugins;
mod providers;
mod theme;
mod tools;
mod tui;
mod usage;

use agent::session::{print_sessions, Session};
use agent::{AgentEvent, AgentRunner, ApprovalDecision, PermissionMode, SharedMode};
use api::ApiClient;
use auth::{auth_status, login_interactive, logout, resolve_api_key, save_api_key};
use clap::Parser;
use cli::{AuthCmd, Cli, Commands};
use config::{load_config, Config};
use error::Result;
use std::collections::HashSet;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use usage::{print_usage_summary, UsageTracker};

#[tokio::main]
async fn main() {
    init_tracing();

    if let Err(e) = real_main().await {
        theme::print_err(&e.to_string());
        std::process::exit(1);
    }
}

/// Log to `~/.nur/nur.log` (never stderr) so ratatui's alternate screen is not
/// painted over by `syntect` / `tui-markdown` WARN noise — that was showing up
/// as garbled text in the input box on a fresh session.
fn init_tracing() {
    use std::fs::OpenOptions;
    use std::sync::Mutex;

    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        // Default: warn for us, hush syntax-highlighter misses (empty fences).
        tracing_subscriber::EnvFilter::new("warn,syntect=error,tui_markdown=error")
    });

    let log_path = config::nur_home().join("nur.log");
    let _ = std::fs::create_dir_all(config::nur_home());
    match OpenOptions::new().create(true).append(true).open(&log_path) {
        Ok(file) => {
            let _ = tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer(Mutex::new(file))
                .with_ansi(false)
                .with_target(false)
                .try_init();
        }
        Err(_) => {
            // Last resort: still suppress noisy crates if we must use stderr.
            let _ = tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                        tracing_subscriber::EnvFilter::new(
                            "error,syntect=error,tui_markdown=error",
                        )
                    }),
                )
                .with_target(false)
                .try_init();
        }
    }
}

async fn real_main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::Auth { action }) => {
            match action {
                AuthCmd::Login { key } => login_interactive(key.clone())?,
                AuthCmd::Status => auth_status()?,
                AuthCmd::Logout { revoke } => {
                    logout(*revoke)?;
                    if *revoke {
                        theme::print_ok(
                            "logged out (local ~/.nur/auth.json removed; see revoke notes above)",
                        );
                    } else {
                        theme::print_ok("logged out (removed ~/.nur/auth.json)");
                    }
                }
            }
            return Ok(());
        }
        Some(Commands::Usage) => {
            print_usage_summary()?;
            return Ok(());
        }
        Some(Commands::Sessions { limit }) => {
            print_sessions(*limit)?;
            return Ok(());
        }
        Some(Commands::InstallHook) => {
            ade::install_orca_hook()?;
            return Ok(());
        }
        Some(Commands::Install | Commands::SelfInstall) => {
            // Explicit one-stop install (same as double-clicking the release EXE).
            bootstrap::run_full_install()?;
            return Ok(());
        }
        Some(Commands::Update) => {
            bootstrap::run_update()?;
            return Ok(());
        }
        Some(Commands::Doctor) => {
            run_doctor()?;
            return Ok(());
        }
        Some(Commands::Ecosystem { action }) => {
            match action {
                cli::EcosystemCmd::Ensure { force } => {
                    theme::print_info(
                        "provisioning graphify · plur · ruflo · browser · excalidraw · skills…",
                    );
                    let st = ecosystem::ensure_ecosystem(*force);
                    println!("{}", st.report());
                    if !(st.graphify.available && st.plur.available && st.ruflo.available) {
                        // Partial success still exits 0 if at least one works;
                        // exit 1 only when everything failed.
                        if !st.graphify.available && !st.plur.available && !st.ruflo.available {
                            return Err(error::MuseError::Other(
                                "ecosystem ensure failed — install Node.js 20+ and uv, then re-run nur ecosystem ensure"
                                    .into(),
                            ));
                        }
                        theme::print_info("partial ecosystem — missing components noted above");
                    } else if !st.excalidraw.available {
                        theme::print_info(
                            "core ready; excalidraw-cli missing — needs Node/npm (npm i -g excalidraw-cli)",
                        );
                    } else {
                        theme::print_ok("ecosystem ready");
                    }
                }
                cli::EcosystemCmd::Status => {
                    print!("{}", ecosystem::quick_status());
                }
            }
            return Ok(());
        }
        Some(Commands::Browser { action }) => {
            match action {
                cli::BrowserCmd::Setup => run_browser_setup(true)?,
                cli::BrowserCmd::Status => {
                    theme::print_info("browser tool status");
                    print!("{}", ecosystem::browser_setup::setup_summary());
                    if ecosystem::find_bin("agent-browser-cli").is_some() {
                        theme::print_ok("agent-browser-cli present");
                    } else {
                        theme::print_info(
                            "agent-browser-cli not on PATH — run `nur ecosystem ensure`",
                        );
                    }
                }
            }
            return Ok(());
        }
        Some(Commands::Plugins { action }) => {
            run_plugins_cli(action.as_ref())?;
            return Ok(());
        }
        Some(Commands::Local { action }) => {
            // Local models need no API key/auth — handle before the auth path.
            local::run_local(action).await?;
            return Ok(());
        }
        None => {
            // Interactive launch: one-stop install FIRST when needed (release EXE
            // or never bootstrapped). Never open the TUI while packs are still
            // installing in the background for a first-time machine.
            if bootstrap::should_bootstrap_on_launch() {
                bootstrap::run_full_install()?;
                // Release artifact → re-exec the installed `nur` for a clean TUI.
                if bootstrap::looks_like_release_artifact()
                    && !bootstrap::is_running_from_install()
                {
                    bootstrap::reexec_installed_tui()?;
                    return Ok(());
                }
            }
        }
        _ => {}
    }

    let mut cfg = load_config()?;
    if let Some(m) = &cli.model {
        cfg.model = m.clone();
    } else if let Ok(m) = std::env::var("NUR_MODEL")
        .or_else(|_| std::env::var("META_MODEL"))
        .or_else(|_| std::env::var("MUSE_MODEL"))
    {
        if !m.trim().is_empty() {
            cfg.model = m;
        }
    }
    if let Some(e) = &cli.effort {
        cfg.reasoning_effort = e.clone();
    }
    if let Some(t) = cli.max_turns {
        cfg.max_turns = t;
    }

    // Interactive TUI can start without a key and prompt `/login`; headless
    // `run` still needs a key up front (no place to prompt).
    // Resolve against the *active* provider so a Grok OAuth session is not
    // silently reused against OpenAI/etc. after a config switch.
    let interactive = cli.command.is_none();
    let api_key = match auth::resolve_api_key_for(Some(cfg.provider.as_str())) {
        Ok(k) => k,
        Err(error::MuseError::NotAuthenticated) => {
            let env_key = std::env::var("NUR_API_KEY")
                .or_else(|_| std::env::var("META_API_KEY"))
                .or_else(|_| std::env::var("MODEL_API_KEY"))
                .or_else(|_| std::env::var("MUSE_API_KEY"))
                .ok()
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty());
            if let Some(k) = env_key {
                let _ = save_api_key(&k);
                k
            } else if interactive {
                // Empty key → TUI opens signed-out and auto-opens /login.
                String::new()
            } else {
                return Err(error::MuseError::NotAuthenticated);
            }
        }
        Err(e) => {
            // Provider mismatch: interactive → open TUI unauthed; headless → fail.
            let msg = e.to_string();
            if interactive && msg.contains("mismatch") {
                theme::print_info(&msg);
                String::new()
            } else if interactive {
                String::new()
            } else {
                return Err(e);
            }
        }
    };

    let explicit_cwd = cli.cwd.is_some();
    let requested = cli
        .cwd
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);

    // Safe workspace: auto-fallback when user starts at C:\ / / (common on Windows).
    let (cwd, why) = tools::resolve_safe_workspace(&requested, explicit_cwd)?;
    // Don't print workspace tips to stdout before the TUI — that races with
    // alternate-screen enter and can look like a crash (tips, then shell).
    // Hand the note into the TUI instead.
    let workspace_note = why.map(|reason| {
        format!(
            "workspace  {}  ·  {reason}\n\
             tip  /cd into your repo, or set NUR_CWD for a default",
            cwd.display()
        )
    });
    // Enter the workspace so relative paths / shell feel natural.
    let _ = std::env::set_current_dir(&cwd);
    let cwd_str = cwd.display().to_string();

    // Honor the saved provider's API shape (Responses vs Chat Completions).
    let chat_mode = providers::by_id(&cfg.provider)
        .map(|p| p.style == providers::ApiStyle::ChatCompletions)
        .unwrap_or(false);
    let client = ApiClient::new(&cfg.base_url, &api_key)?.with_chat_completions(chat_mode);

    let mut session = if let Some(id) = &cli.resume {
        theme::print_info(&format!("resuming session {id}…"));
        Session::load(id)?
    } else if cli.continue_session {
        theme::print_info("continuing last session for this directory…");
        Session::continue_for_cwd(&cwd_str)?
    } else {
        Session::new(&cfg.model, &cwd_str)
    };

    // Keep model in sync if user overrode
    if session.model != cfg.model {
        session.model = cfg.model.clone();
    }
    session.cwd = cwd_str.clone();

    let mut usage = UsageTracker::new(session.id.clone(), cfg.model.clone(), cwd.clone());
    // Seed tracker with prior session usage so host panel totals stay cumulative
    if session.usage.total_tokens > 0 {
        usage.seed_session(session.usage.clone());
    }

    let home_s = config::nur_home().display().to_string();
    let status_s = config::status_path().display().to_string();
    let usage_s = config::usage_log_path().display().to_string();
    // Prefer NUR_*; keep META_* / MUSE_* aliases so host hooks don't brick.
    for (nur_k, meta_k, muse_k, val) in [
        (
            "NUR_STATUS_PATH",
            "META_STATUS_PATH",
            "MUSE_STATUS_PATH",
            status_s.as_str(),
        ),
        (
            "NUR_USAGE_LOG_PATH",
            "META_USAGE_LOG_PATH",
            "MUSE_USAGE_LOG_PATH",
            usage_s.as_str(),
        ),
        (
            "NUR_SESSION_ID",
            "META_SESSION_ID",
            "MUSE_SESSION_ID",
            session.id.as_str(),
        ),
        ("NUR_MODEL", "META_MODEL", "MUSE_MODEL", cfg.model.as_str()),
        (
            "NUR_PROVIDER",
            "META_PROVIDER",
            "MUSE_PROVIDER",
            cfg.provider.as_str(),
        ),
        ("NUR_HOME", "META_HOME", "MUSE_HOME", home_s.as_str()),
    ] {
        std::env::set_var(nur_k, val);
        std::env::set_var(meta_k, val);
        std::env::set_var(muse_k, val);
    }
    // Ruflo global memory (shared store under nur home).
    std::env::set_var(
        "CLAUDE_FLOW_DB_PATH",
        ecosystem::ruflo_db_path().display().to_string(),
    );
    std::env::set_var(
        "CLAUDE_FLOW_MEMORY_PATH",
        ecosystem::ruflo_home().display().to_string(),
    );

    ade::write_ade_manifest(&session.id, &cfg.model, &cwd_str, usage.session_usage());
    let _ = session.save();

    // First open already ran a full foreground install when needed. Background
    // ensure is TTL repair only (skips fast when the marker is fresh).
    let eco_summary = ecosystem::launch_snapshot();
    if cfg.ecosystem_auto_ensure {
        std::thread::spawn(|| {
            let _ = ecosystem::ensure_ecosystem(false);
        });
    }

    let start_mode = if cli.yes {
        PermissionMode::Auto
    } else if let Some(m) = &cli.mode {
        PermissionMode::parse(m).unwrap_or(PermissionMode::Manual)
    } else {
        PermissionMode::Manual
    };
    let permission_mode = SharedMode::new(start_mode);

    match &cli.command {
        Some(Commands::Run { prompt, yes }) => {
            let prompt = prompt.join(" ");
            if *yes {
                permission_mode.set(PermissionMode::Auto);
            }
            run_headless(
                client,
                cfg,
                cwd,
                session,
                usage,
                &prompt,
                permission_mode,
                cli.verbose,
            )
            .await?;
        }
        None if cli.continuous => {
            let goal = cli
                .prompt
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let Some(goal) = goal else {
                return Err(error::MuseError::Other(
                    "--continuous needs a goal, e.g.  nur \"keep the tests green\" --continuous"
                        .into(),
                ));
            };
            run_continuous(
                client,
                cfg,
                cwd,
                session,
                usage,
                &goal,
                permission_mode,
                cli.verbose,
                cli.max_iters,
            )
            .await?;
        }
        None => {
            // Compact host tab title from first prompt (implementation detail, not marketing).
            let seed = cli
                .prompt
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .or_else(|| {
                    session
                        .messages
                        .iter()
                        .find(|m| m.role == "user")
                        .map(|m| m.content.clone())
                });
            ade::set_terminal_title(&ade::session_window_title(
                seed.as_deref().unwrap_or("ready"),
            ));
            tui::run_tui(
                client,
                cfg,
                cwd,
                permission_mode,
                session,
                usage,
                cli.prompt.clone(),
                eco_summary,
                workspace_note,
            )
            .await?;
        }
        Some(Commands::Gateway { token, chat }) => {
            gateway::run_gateway(client, cfg, cwd, session, usage, token.clone(), *chat).await?;
        }
        Some(Commands::Bench { action }) => {
            bench::run_bench(action, client, cfg, cwd).await?;
        }
        Some(Commands::Auth { .. })
        | Some(Commands::Usage)
        | Some(Commands::Sessions { .. })
        | Some(Commands::InstallHook)
        | Some(Commands::Install)
        | Some(Commands::SelfInstall)
        | Some(Commands::Update)
        | Some(Commands::Doctor)
        | Some(Commands::Ecosystem { .. })
        | Some(Commands::Browser { .. })
        | Some(Commands::Local { .. })
        | Some(Commands::Plugins { .. }) => unreachable!(),
    }

    Ok(())
}

fn run_plugins_cli(action: Option<&cli::PluginsCmd>) -> Result<()> {
    use cli::PluginsCmd;
    match action {
        None | Some(PluginsCmd::List) => {
            theme::print_info("plugin marketplace");
            println!("{}", plugins::quick_status());
            for r in plugins::marketplace_rows() {
                println!(
                    "  {:<16}  {:<14}  {}",
                    r.id,
                    r.status_badge(),
                    r.name
                );
            }
            println!("\n  nur plugins install <id>");
            println!("  /plugins  in the TUI for the full picker");
        }
        Some(PluginsCmd::Install { id }) => {
            theme::print_info(&format!("installing {id}…"));
            match plugins::install_plugin(id) {
                Ok(msg) => theme::print_ok(&msg),
                Err(e) => return Err(error::MuseError::Other(e)),
            }
        }
        Some(PluginsCmd::Enable { id }) => {
            plugins::set_enabled(id, true).map_err(error::MuseError::Other)?;
            theme::print_ok(&format!("enabled {id}"));
        }
        Some(PluginsCmd::Disable { id }) => {
            plugins::set_enabled(id, false).map_err(error::MuseError::Other)?;
            theme::print_ok(&format!("disabled {id}"));
        }
        Some(PluginsCmd::Uninstall { id }) => match plugins::uninstall_plugin(id) {
            Ok(msg) => theme::print_ok(&msg),
            Err(e) => return Err(error::MuseError::Other(e)),
        },
    }
    Ok(())
}

/// Prepare the real-Chrome `browser` tool: ensure the CLI + extension are
/// staged, detect the default browser, and (for `setup`) open its extensions
/// page so the one-time "load unpacked" step is a single action. Everything
/// else — CLI install, extension files, default-browser targeting — is
/// automatic; the load click is a Chromium security boundary we can't script.
fn run_browser_setup(open: bool) -> Result<()> {
    use ecosystem::browser_setup as bs;
    theme::print_info("browser tool setup");

    // Make sure the CLI is present (provisions it if a package manager exists).
    if ecosystem::find_bin("agent-browser-cli").is_none() {
        theme::print_info("installing agent-browser-cli…");
        let _ = ecosystem::ensure_ecosystem(false);
    }

    // Stage the extension out of the installed package (no download needed).
    let staged = bs::stage_extension_from_cli().or_else(|| {
        let d = bs::staged_extension_dir();
        d.join("manifest.json").is_file().then_some(d)
    });
    let browser = bs::detect_default_browser();
    theme::print_ok(&format!("default browser · {}", browser.label()));

    match &staged {
        Some(dir) => theme::print_ok(&format!("extension staged · {}", dir.display())),
        None => theme::print_err(
            "could not stage the extension — run `nur ecosystem ensure` first, \
             or load it from the agent-browser-cli release zip",
        ),
    }

    if !browser.is_chromium() {
        theme::print_info(
            "your default browser isn't Chromium — the bridge needs \
             Arc / Chrome / Edge / Brave / Chromium set as default",
        );
        return Ok(());
    }

    if let Some(dir) = &staged {
        println!();
        theme::print_info(&format!(
            "one-time load in {} (Chromium security requires this click):",
            browser.label()
        ));
        println!("    1. open  {}", browser.extensions_url());
        println!("    2. toggle  Developer mode  (top-right)");
        println!("    3. click  Load unpacked  →  choose:");
        println!("         {}", dir.display());
        println!("    4. keep at least one normal web tab open");
        // Copy the path so the folder picker is a paste away.
        if crate::ade::copy_to_clipboard(&dir.display().to_string()) {
            theme::print_ok("extension path copied to clipboard");
        }
        if open {
            // Open the extensions page in the default browser. `chrome://` URLs
            // only resolve inside a Chromium browser, so hand it to the OS
            // opener which routes to the default browser.
            let _ = crate::open_uri::open(browser.extensions_url());
            theme::print_ok(&format!("opened {} in {}", browser.extensions_url(), browser.label()));
        }
    }
    println!();
    theme::print_ok("after loading once, the `browser` tool works in every session");
    Ok(())
}

/// Headless health check for install, auth, config, and ecosystem.
fn run_doctor() -> Result<()> {
    theme::print_info(&format!("nur doctor · v{}", env!("CARGO_PKG_VERSION")));
    println!();

    // Binary / PATH
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "(unknown)".into());
    theme::print_ok(&format!("binary  {exe}"));

    // Config
    match load_config() {
        Ok(cfg) => {
            let cost_cap = cfg
                .max_session_cost_usd
                .map(|c| format!("${c:.2}"))
                .unwrap_or_else(|| "∞$".into());
            let tok_cap = cfg
                .max_session_tokens
                .map(|t| t.to_string())
                .unwrap_or_else(|| "∞tok".into());
            theme::print_ok(&format!(
                "config  model={} effort={} max_turns={} budget={}/{}  ({})",
                cfg.model,
                cfg.reasoning_effort,
                cfg.max_turns,
                cost_cap,
                tok_cap,
                config::config_path().display()
            ));
        }
        Err(e) => theme::print_err(&format!("config  {e}")),
    }

    // Auth (never print key)
    match resolve_api_key() {
        Ok(k) => {
            let tip: String = k.chars().rev().take(4).collect::<String>().chars().rev().collect();
            theme::print_ok(&format!("auth    key set (…{tip})"));
        }
        Err(_) => theme::print_err("auth    not set — run: nur auth login"),
    }

    // Paths
    theme::print_ok(&format!("home    {}", config::meta_home().display()));
    theme::print_ok(&format!("status  {}", config::status_path().display()));
    theme::print_ok(&format!("usage   {}", config::usage_log_path().display()));
    theme::print_ok(&format!("sessions {}", config::sessions_dir().display()));

    // Ecosystem
    println!();
    theme::print_info("ecosystem");
    print!("{}", ecosystem::quick_status());

    // Plugins marketplace
    println!();
    theme::print_info("plugins");
    print!("{}", plugins::quick_status());

    // Shell backend
    println!();
    let sh = tools::shell_backend();
    theme::print_ok(&format!("shell   {}", sh.label));

    // Optional tools on PATH
    for name in ["rg", "git", "node", "npm", "uv", "bun", "ffmpeg"] {
        let found = which_bin(name);
        if let Some(p) = found {
            theme::print_ok(&format!("{name:<7} {p}"));
        } else if name == "ffmpeg" {
            theme::print_info("ffmpeg  not on PATH (optional — extract_frames / design-from-video)");
        } else if name == "bun" {
            theme::print_info("bun     not on PATH (optional — omp coding-agent backend)");
        } else {
            theme::print_info(&format!("{name:<7} not on PATH"));
        }
    }
    theme::print_ok("vision  look · extract_frames (input_image / input_video)");

    // Binary integrity (written by install.ps1 / install.sh)
    println!();
    let hash_path = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local")
        .join("bin")
        .join("nur.sha256");
    if hash_path.is_file() {
        if let (Ok(expected_line), Ok(exe)) = (
            std::fs::read_to_string(&hash_path),
            std::env::current_exe(),
        ) {
            let expected = expected_line
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_lowercase();
            match file_sha256(&exe) {
                Ok(actual) if !expected.is_empty() && actual == expected => {
                    theme::print_ok(&format!("sha256  {actual}  (matches install record)"));
                }
                Ok(actual) if !expected.is_empty() => {
                    theme::print_err(&format!(
                        "sha256  {actual}  ≠ recorded {expected}  (re-run install)"
                    ));
                }
                Ok(actual) => theme::print_info(&format!("sha256  {actual}")),
                Err(e) => theme::print_info(&format!("sha256  (could not hash: {e})")),
            }
        }
    } else {
        theme::print_info("sha256  no nur.sha256 next to install (optional integrity record)");
    }

    println!();
    theme::print_ok("doctor complete");
    Ok(())
}

fn file_sha256(path: &std::path::Path) -> std::io::Result<String> {
    // Lightweight: use Windows certutil / shasum via shell when available.
    #[cfg(windows)]
    {
        let out = std::process::Command::new("certutil")
            .args(["-hashfile", &path.display().to_string(), "SHA256"])
            .output()?;
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            let t = line.trim();
            if t.len() == 64 && t.chars().all(|c| c.is_ascii_hexdigit()) {
                return Ok(t.to_lowercase());
            }
        }
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "certutil hash parse failed",
        ));
    }
    #[cfg(not(windows))]
    {
        let out = std::process::Command::new("shasum")
            .args(["-a", "256"])
            .arg(path)
            .output()
            .or_else(|_| {
                std::process::Command::new("sha256sum").arg(path).output()
            })?;
        let text = String::from_utf8_lossy(&out.stdout);
        let hash = text
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_lowercase();
        if hash.len() == 64 {
            Ok(hash)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "sha256 parse failed",
            ))
        }
    }
}

fn which_bin(name: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for cand in [
            dir.join(name),
            dir.join(format!("{name}.exe")),
            dir.join(format!("{name}.cmd")),
        ] {
            if cand.is_file() {
                return Some(cand.display().to_string());
            }
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
async fn run_headless(
    client: ApiClient,
    cfg: Config,
    cwd: PathBuf,
    session: Session,
    usage: UsageTracker,
    prompt: &str,
    permission_mode: SharedMode,
    verbose: bool,
) -> Result<()> {
    ade::set_terminal_title(&ade::session_window_title(prompt));
    let runner = Arc::new(AgentRunner {
        client,
        config: cfg,
        cwd: cwd.clone(),
        permission_mode,
        verbose,
        approved_tools: Arc::new(Mutex::new(HashSet::new())),
        tools: tools::ToolHost::default(),
        permissions: agent::SharedPermissions::load(&cwd),
        hooks: agent::hooks::HooksConfig::load(),
        is_subagent: false,
    });

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel = tokio_util::sync::CancellationToken::new();
    agent::spawn_turn(
        runner,
        session,
        usage,
        prompt.to_string(),
        tx,
        cancel.clone(),
    );

    let mut streamed = false;
    let mut midline = false;
    let mut final_result: std::result::Result<String, String> = Ok(String::new());
    let mut final_usage: Option<Box<UsageTracker>> = None;

    while let Some(ev) = rx.recv().await {
        // Streamed deltas leave the cursor mid-line; break before other output.
        if midline && !matches!(ev, AgentEvent::TextDelta(_)) {
            println!();
            midline = false;
        }
        match ev {
            AgentEvent::TextDelta(d) => {
                streamed = true;
                midline = !d.ends_with('\n');
                print!("{d}");
                let _ = std::io::stdout().flush();
            }
            AgentEvent::AssistantMessage(m) => {
                if verbose {
                    println!("{m}");
                }
            }
            AgentEvent::ReasoningDelta(_) => {}
            AgentEvent::Status(s) => {
                if verbose {
                    theme::print_info(&s);
                }
            }
            AgentEvent::ToolStart { name, args, .. } => {
                if verbose {
                    theme::print_tool(&name, &truncate_line(&args, 120));
                }
            }
            AgentEvent::ToolEnd {
                name, result, ok, ..
            } => {
                if verbose {
                    let tag = if ok { "done" } else { "failed" };
                    theme::print_info(&format!("{name} {tag}: {}", truncate_line(&result, 160)));
                }
            }
            AgentEvent::ApprovalRequest {
                name,
                args,
                respond,
            } => {
                // Interactive terminal prompt (headless without -y).
                let decision = tokio::task::spawn_blocking(move || {
                    eprintln!();
                    theme::print_tool(&name, &truncate_line(&args, 200));
                    eprint!("  approve? [y]es / [a]lways / [N]o: ");
                    let mut line = String::new();
                    let _ = std::io::stdin().read_line(&mut line);
                    match line.trim().to_lowercase().as_str() {
                        "y" | "yes" => ApprovalDecision::Approve,
                        "a" | "always" => ApprovalDecision::ApproveAlways,
                        _ => ApprovalDecision::Deny,
                    }
                })
                .await
                .unwrap_or(ApprovalDecision::Deny);
                let _ = respond.send(decision);
            }
            AgentEvent::Usage { .. } => {}
            AgentEvent::TodosChanged(text) => {
                if verbose {
                    theme::print_info(&format!("todos\n{text}"));
                }
            }
            AgentEvent::PlanSubmitted(text) => {
                if verbose {
                    theme::print_info(&format!("plan\n{text}"));
                }
            }
            AgentEvent::Done { usage, result, .. } => {
                final_usage = Some(usage);
                final_result = result;
                break;
            }
        }
    }

    match final_result {
        Ok(text) => {
            if !streamed && !text.is_empty() {
                println!("{text}");
            }
        }
        Err(e) => return Err(error::MuseError::Other(e)),
    }

    if let Some(usage) = final_usage {
        let u = usage.session_usage();
        if verbose {
            eprintln!(
                "\n--- usage: in={} out={} total={} ~${:.6} ---",
                u.input_tokens,
                u.output_tokens,
                u.total_tokens,
                u.estimated_cost_usd()
            );
            eprintln!("status: {}", config::status_path().display());
        }
    }
    Ok(())
}

/// Continuous / sovereign mode: loop headless turns on one session toward a
/// goal until the model replies `DONE`, Ctrl+C, or `--max-iters`. Ported from
/// wizard's `--continuous`. Auto-approves tools (nur is sandboxed by default);
/// the per-turn agent loop auto-compacts context so long missions stay bounded.
#[allow(clippy::too_many_arguments)]
async fn run_continuous(
    client: ApiClient,
    cfg: Config,
    cwd: PathBuf,
    session: Session,
    usage: UsageTracker,
    goal: &str,
    permission_mode: SharedMode,
    verbose: bool,
    max_iters: u32,
) -> Result<()> {
    permission_mode.set(PermissionMode::Auto);
    ade::set_terminal_title(&ade::session_window_title(goal));

    let runner = Arc::new(AgentRunner {
        client,
        config: cfg,
        cwd: cwd.clone(),
        permission_mode,
        verbose,
        approved_tools: Arc::new(Mutex::new(HashSet::new())),
        tools: tools::ToolHost::default(),
        permissions: agent::SharedPermissions::load(&cwd),
        hooks: agent::hooks::HooksConfig::load(),
        is_subagent: false,
    });

    let cancel = tokio_util::sync::CancellationToken::new();
    {
        let c = cancel.clone();
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            c.cancel();
        });
    }

    theme::print_info(&format!("continuous · goal: {goal}"));
    theme::print_info("auto-approving tools · Ctrl+C stops after the current step");
    if max_iters > 0 {
        theme::print_info(&format!("continuous · max {max_iters} steps"));
    }

    // Held in Options so the loop stays move-safe: each turn takes ownership and
    // hands them back via `Done`. If a turn's task ever dies without `Done`, they
    // stay `None` and the final save is simply skipped.
    let mut session = Some(session);
    let mut usage = Some(usage);
    let mut iter = 0u32;
    let mut errors_in_a_row = 0u32;

    loop {
        if cancel.is_cancelled() {
            theme::print_info("continuous · stopped (Ctrl+C)");
            break;
        }
        iter += 1;
        if max_iters > 0 && iter > max_iters {
            theme::print_info(&format!("continuous · reached max {max_iters} steps"));
            break;
        }
        theme::print_info(&format!("── step {iter} ──"));

        let prompt = agent::continuous::continuous_prompt(goal, iter);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (Some(sess), Some(usg)) = (session.take(), usage.take()) else {
            break;
        };
        agent::spawn_turn(runner.clone(), sess, usg, prompt, tx, cancel.clone());

        let mut midline = false;
        let mut done: Option<(
            Box<Session>,
            Box<UsageTracker>,
            std::result::Result<String, String>,
            bool,
        )> = None;
        while let Some(ev) = rx.recv().await {
            if midline && !matches!(ev, AgentEvent::TextDelta(_)) {
                println!();
                midline = false;
            }
            match ev {
                AgentEvent::TextDelta(d) => {
                    midline = !d.ends_with('\n');
                    print!("{d}");
                    let _ = std::io::stdout().flush();
                }
                AgentEvent::Status(s) => {
                    if verbose {
                        theme::print_info(&s);
                    }
                }
                AgentEvent::ToolStart { name, args, .. } => {
                    if verbose {
                        theme::print_tool(&name, &truncate_line(&args, 120));
                    }
                }
                AgentEvent::ToolEnd {
                    name, result, ok, ..
                } => {
                    if verbose {
                        let tag = if ok { "done" } else { "failed" };
                        theme::print_info(&format!("{name} {tag}: {}", truncate_line(&result, 160)));
                    }
                }
                AgentEvent::Done {
                    session: s,
                    usage: u,
                    result,
                    interrupted,
                } => {
                    done = Some((s, u, result, interrupted));
                    break;
                }
                _ => {}
            }
        }

        let Some((s, u, result, interrupted)) = done else {
            break;
        };
        session = Some(*s);
        usage = Some(*u);

        match result {
            Ok(text) => {
                errors_in_a_row = 0;
                if interrupted {
                    theme::print_info("continuous · interrupted");
                    break;
                }
                if agent::continuous::is_done(&text) {
                    theme::print_ok("continuous · goal complete (DONE)");
                    break;
                }
            }
            Err(e) => {
                errors_in_a_row += 1;
                theme::print_info(&format!("continuous · step error: {e}"));
                if errors_in_a_row >= 3 {
                    theme::print_info("continuous · 3 errors in a row — stopping");
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_secs(2 * errors_in_a_row as u64)).await;
            }
        }
    }

    if let Some(s) = session {
        let _ = s.save();
    }
    Ok(())
}

fn truncate_line(s: &str, max: usize) -> String {
    let s = s.replace('\n', " ");
    if s.chars().count() <= max {
        s
    } else {
        let t: String = s.chars().take(max).collect();
        format!("{t}…")
    }
}
