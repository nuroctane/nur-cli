use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "meta",
    version,
    about = "Meta CLI (unofficial) — fully loaded Meta Model API coding agent · TUI · tools · Graphify/PLUR/Ruflo · 800+ skills",
    long_about = "Meta CLI — unofficial, fully loaded community coding agent for Meta Model API.\n\n\
What you get:\n\
  · Streaming Meta-blue TUI — duration chips, expandable thought/tool cards,\n\
    click-to-peek, drag-select + always-on scrollbar, sessions browser, sticky prompt\n\
  · Real agent harness — manual/plan/auto modes, tool loop, subagents, todos,\n\
    auto-compact, approvals with mini-diff, Esc cancel, Shift+Tab live mode flip\n\
  · Native tools — read/edit/shell/web/git + knowledge stack\n\
  · Ecosystem auto-wired — Graphify · PLUR · Ruflo · Executor · 800+ skills · AKM\n\
  · Hardened by default — sandbox, bash denylist, SSRF blocks, API retries,\n\
    prompt-cache keys, atomic session IO, install SHA-256, meta doctor\n\
  · Host panels — live status.json / usage.jsonl for ADE/Orca-style dashboards\n\n\
Model: any Meta Model API id via --model / /model / config (default from config).\n\
Secrets stay in ~/.meta/ only.  Not affiliated with Meta Platforms, Inc.\n\
Repo: github.com/nuroctane/meta-cli  ·  Invoke as: meta  (alias: muse)"
)]
pub struct Cli {
    /// Initial prompt for interactive session
    #[arg(value_name = "PROMPT")]
    pub prompt: Option<String>,

    /// Meta Model API model id (default from config)
    #[arg(short, long, env = "META_MODEL")]
    pub model: Option<String>,

    /// Working directory
    #[arg(long)]
    pub cwd: Option<String>,

    /// Auto-approve tools (sets permission mode to auto)
    #[arg(long, short = 'y', global = true)]
    pub yes: bool,

    /// Permission mode: manual | plan | auto  (Shift+Tab cycles in TUI)
    #[arg(long, global = true, value_name = "MODE")]
    pub mode: Option<String>,

    /// Reasoning effort: minimal|low|medium|high|xhigh
    #[arg(long)]
    pub effort: Option<String>,

    /// Max agent turns per prompt
    #[arg(long)]
    pub max_turns: Option<u32>,

    /// Verbose tool logging (headless)
    #[arg(long, short, global = true)]
    pub verbose: bool,

    /// Continue the most recent session for this cwd
    #[arg(short = 'c', long)]
    pub continue_session: bool,

    /// Resume a specific session id (full UUID or unique prefix)
    #[arg(short = 'r', long = "resume")]
    pub resume: Option<String>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run a single agent turn headlessly (prints final answer)
    Run {
        /// Prompt text
        #[arg(required = true)]
        prompt: Vec<String>,
        /// Auto-approve tools
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Authentication against Meta Model API
    Auth {
        #[command(subcommand)]
        action: AuthCmd,
    },
    /// Show last known token usage (status / usage log paths)
    Usage,
    /// List recent sessions
    Sessions {
        /// Max rows
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Install Orca agent hook for usage/status reporting
    InstallHook,
    /// Diagnose install, auth, config, and ecosystem readiness
    Doctor,
    /// Graphify · PLUR · Ruflo ecosystem (auto-provisioned on open)
    Ecosystem {
        #[command(subcommand)]
        action: EcosystemCmd,
    },
}

#[derive(Subcommand, Debug)]
pub enum EcosystemCmd {
    /// Install/repair graphify, plur, ruflo + skills (also runs automatically on open)
    Ensure {
        /// Force re-install even if marker is fresh
        #[arg(long, short)]
        force: bool,
    },
    /// Show ecosystem readiness
    Status,
}

#[derive(Subcommand, Debug)]
pub enum AuthCmd {
    /// Save API key to ~/.meta/auth.json
    Login {
        /// API key (optional; prompts if omitted)
        #[arg(long)]
        key: Option<String>,
    },
    /// Show auth status (never prints full key)
    Status,
    /// Remove saved key
    Logout,
}
