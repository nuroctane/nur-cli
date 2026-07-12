mod ade;
mod agent;
mod api;
mod auth;
mod cli;
mod config;
mod ecosystem;
mod error;
mod theme;
mod tools;
mod tui;
mod usage;

use agent::session::{print_sessions, Session};
use agent::{AgentEvent, AgentRunner, ApprovalDecision, PermissionMode, SharedMode};
use api::MetaClient;
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
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_target(false)
        .init();

    if let Err(e) = real_main().await {
        theme::print_err(&e.to_string());
        std::process::exit(1);
    }
}

async fn real_main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::Auth { action }) => {
            match action {
                AuthCmd::Login { key } => login_interactive(key.clone())?,
                AuthCmd::Status => auth_status()?,
                AuthCmd::Logout => {
                    logout()?;
                    theme::print_ok("logged out (removed ~/.muse/auth.json)");
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
        Some(Commands::Ecosystem { action }) => {
            match action {
                cli::EcosystemCmd::Ensure { force } => {
                    theme::print_info("provisioning graphify · plur · ruflo (one-shot)…");
                    let st = ecosystem::ensure_ecosystem(*force);
                    println!("{}", st.report());
                    if !(st.graphify.available && st.plur.available && st.ruflo.available) {
                        // Partial success still exits 0 if at least one works;
                        // exit 1 only when everything failed.
                        if !st.graphify.available && !st.plur.available && !st.ruflo.available {
                            return Err(error::MuseError::Other(
                                "ecosystem ensure failed — install Node.js 20+ and uv, then re-run meta ecosystem ensure"
                                    .into(),
                            ));
                        }
                        theme::print_info("partial ecosystem — missing components noted above");
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
        _ => {}
    }

    let api_key = match resolve_api_key() {
        Ok(k) => k,
        Err(_) => {
            if let Ok(k) = std::env::var("MODEL_API_KEY").or_else(|_| std::env::var("MUSE_API_KEY"))
            {
                if !k.trim().is_empty() {
                    let _ = save_api_key(k.trim());
                    k
                } else {
                    return Err(error::MuseError::NotAuthenticated);
                }
            } else {
                return Err(error::MuseError::NotAuthenticated);
            }
        }
    };

    let mut cfg = load_config()?;
    if let Some(m) = &cli.model {
        cfg.model = m.clone();
    } else if let Ok(m) = std::env::var("MUSE_MODEL").or_else(|_| std::env::var("META_MODEL")) {
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

    let explicit_cwd = cli.cwd.is_some();
    let requested = cli
        .cwd
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);

    // Safe workspace: auto-fallback when user starts at C:\ / / (common on Windows).
    let (cwd, why) = tools::resolve_safe_workspace(&requested, explicit_cwd)?;
    if let Some(reason) = why {
        theme::print_info(&format!(
            "workspace: {}  ·  {reason}",
            cwd.display()
        ));
        theme::print_info("tip: cd into your repo, or set user env META_CWD for a default");
    }
    // Enter the workspace so relative paths / shell feel natural.
    let _ = std::env::set_current_dir(&cwd);
    let cwd_str = cwd.display().to_string();

    let client = MetaClient::new(&cfg.base_url, &api_key)?;

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
    // Seed tracker with prior session usage so ADE totals stay cumulative
    if session.usage.total_tokens > 0 {
        usage.seed_session(session.usage.clone());
    }

    std::env::set_var("MUSE_STATUS_PATH", config::status_path().display().to_string());
    std::env::set_var(
        "MUSE_USAGE_LOG_PATH",
        config::usage_log_path().display().to_string(),
    );
    std::env::set_var("MUSE_SESSION_ID", &session.id);
    std::env::set_var("MUSE_MODEL", &cfg.model);
    std::env::set_var("MUSE_PROVIDER", "meta");
    std::env::set_var("MUSE_HOME", config::muse_home().display().to_string());
    // Ruflo global memory (so child CLIs share Meta's store without polluting projects).
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

    // Never block launch on ecosystem install (npm/uv/skill packs can take minutes).
    // Snapshot whatever is already provisioned; repair in a background thread.
    let eco_summary = ecosystem::launch_snapshot();
    std::thread::spawn(|| {
        let _ = ecosystem::ensure_ecosystem(false);
    });

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
        None => {
            ade::set_terminal_title(&format!(
                "meta · {}",
                &session.id[..8.min(session.id.len())]
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
            )
            .await?;
        }
        Some(Commands::Auth { .. })
        | Some(Commands::Usage)
        | Some(Commands::Sessions { .. })
        | Some(Commands::InstallHook)
        | Some(Commands::Ecosystem { .. }) => unreachable!(),
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_headless(
    client: MetaClient,
    cfg: Config,
    cwd: PathBuf,
    session: Session,
    usage: UsageTracker,
    prompt: &str,
    permission_mode: SharedMode,
    verbose: bool,
) -> Result<()> {
    let runner = Arc::new(AgentRunner {
        client,
        config: cfg,
        cwd,
        permission_mode,
        verbose,
        approved_tools: Arc::new(Mutex::new(HashSet::new())),
        tools: tools::ToolHost::default(),
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

fn truncate_line(s: &str, max: usize) -> String {
    let s = s.replace('\n', " ");
    if s.chars().count() <= max {
        s
    } else {
        let t: String = s.chars().take(max).collect();
        format!("{t}…")
    }
}
