use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "meta",
    version,
    about = "Meta CLI (unofficial) — coding agent for Meta Model API",
    long_about = "Meta CLI is an unofficial community coding agent for Meta Model API.\nDefault model: muse-spark-1.1 (switch with --model / /model). Not affiliated with Meta Platforms, Inc.\nRepo: github.com/nuroctane/meta-cli\n\nInvoke as: meta   (alias: muse)"
)]
pub struct Cli {
    /// Initial prompt for interactive session
    #[arg(value_name = "PROMPT")]
    pub prompt: Option<String>,

    /// Meta Model API model id (default from config / muse-spark-1.1)
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
    /// Show last known token usage (ADE-friendly paths)
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
    /// Save API key to ~/.muse/auth.json
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
