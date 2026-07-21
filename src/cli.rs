use clap::{Parser, Subcommand};

// Re-export for main.rs match arms.

#[derive(Parser, Debug)]
#[command(
    name = "nur",
    version,
    about = "NurCLI — multi-provider coding agent · vision · TUI · tools · 800+ skills",
    long_about = "NurCLI — fully loaded multi-provider coding agent.\n\n\
What you get:\n\
  · Streaming Nur-gold TUI — duration chips, expandable thought/tool cards,\n\
    click-to-peek, drag-select + scrollbar, Ctrl+A/C/V, sessions browser\n\
  · Vision — look (images/short video) · extract_frames (ffmpeg keyframes)\n\
  · Real agent harness — manual/plan/auto modes, tools, subagents, todos\n\
  · Ecosystem — Graphify · PLUR · Ruflo · Executor · 800+ skills · AKM\n\
  · Hardened by default — sandbox, denylist, SSRF blocks, atomic ~/.nur IO\n\n\
Providers: OpenAI, Anthropic, xAI, Gemini, Meta Model API, OpenRouter, local Ollama, …\n\
Secrets stay in ~/.nur/ only.\n\
Repo: github.com/nuroctane/nur-cli  ·  Invoke as: nur"
)]
pub struct Cli {
    /// Initial prompt for interactive session
    #[arg(value_name = "PROMPT")]
    pub prompt: Option<String>,

    /// Model id (default from config / provider). Env: NUR_MODEL or META_MODEL.
    #[arg(short, long, env = "NUR_MODEL")]
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

    /// Max agent turns per prompt (`0` = unlimited; default from config is unlimited)
    #[arg(long)]
    pub max_turns: Option<u32>,

    /// Continuous/sovereign mode: run headless turns toward the prompt as a goal,
    /// looping until the model replies DONE, Ctrl+C, or --max-iters. Auto-approves
    /// tools (sandboxed). Example: nur "keep the tests green" --continuous
    #[arg(long)]
    pub continuous: bool,

    /// Continuous mode: stop after N iterations (0 = unlimited).
    #[arg(long, default_value_t = 0)]
    pub max_iters: u32,

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
    /// Authentication (API key / status / logout)
    Auth {
        #[command(subcommand)]
        action: AuthCmd,
    },
    /// Show last known token usage (status / usage log paths)
    Usage,
    /// List recent sessions
    Sessions {
        /// Max rows (0 = all)
        #[arg(long, default_value_t = 0)]
        limit: usize,
    },
    /// Install Orca agent hook for usage/status reporting
    InstallHook,
    /// One-stop install: binary → PATH → prereqs → ecosystem → browser (no TUI)
    Install,
    /// Alias for `nur install`
    SelfInstall,
    /// Pull latest source + rebuild/reinstall full stack (same spirit as the one-liner)
    Update,
    /// Diagnose install, auth, config, and ecosystem readiness
    Doctor,
    /// Graphify · PLUR · Ruflo ecosystem (auto-provisioned on open)
    Ecosystem {
        #[command(subcommand)]
        action: EcosystemCmd,
    },
    /// Set up the real-Chrome `browser` tool for your default browser
    Browser {
        #[command(subcommand)]
        action: BrowserCmd,
    },
    /// Marketplace plugins (install skills into ~/.nur/plugins)
    Plugins {
        #[command(subcommand)]
        action: Option<PluginsCmd>,
    },
    /// Run headless as a Telegram bot — each message is an agent turn in this project
    Gateway {
        /// Bot token (else $TELEGRAM_BOT_TOKEN)
        #[arg(long)]
        token: Option<String>,
        /// Restrict to a single chat id (else $TELEGRAM_CHAT_ID; unset = allow anyone)
        #[arg(long)]
        chat: Option<i64>,
    },
    /// Managed local models — bundle llama.cpp + run a GGUF locally (no API key)
    Local {
        #[command(subcommand)]
        action: LocalCmd,
    },
    /// Benchmark models on your own tasks (record trajectories, replay + score)
    Bench {
        #[command(subcommand)]
        action: BenchCmd,
    },
}

#[derive(Subcommand, Debug)]
pub enum LocalCmd {
    /// Fetch llama.cpp + a GGUF sized to this machine, then start the local server
    Up {
        /// Tier (small|medium|large) or a direct .gguf URL. Default: sized to RAM.
        model: Option<String>,
    },
    /// Stop the managed llama-server
    Down,
    /// Show managed-local status (server · model · llama.cpp)
    Status,
    /// List the built-in model tiers
    Models,
}

#[derive(Subcommand, Debug)]
pub enum BenchCmd {
    /// Record a task: nur bench add <name> "<prompt>" [--check "<cmd>"]
    Add {
        /// Short task name
        name: String,
        /// The task prompt (quote it)
        #[arg(required = true)]
        prompt: Vec<String>,
        /// Shell check deciding pass/fail (exit 0 = pass), run in the worktree after the task
        #[arg(long)]
        check: Option<String>,
    },
    /// List recorded tasks
    List,
    /// Remove a recorded task
    Remove {
        /// Task name
        name: String,
    },
    /// Replay a task across models in isolated git worktrees and score them
    Run {
        /// Task name (or "all")
        name: String,
        /// Models to compare (comma-separated); default = the active model
        #[arg(long)]
        models: Option<String>,
    },
    /// GEPA: evolve the standing instruction against your recorded tasks.
    /// Scores candidates on the real bench (pass rate / seconds / tokens),
    /// keeps the Pareto front, and asks the model to improve front members.
    /// Costs real tokens — every candidate is a full agent run per task.
    Optimize {
        /// Task name (or "all")
        name: String,
        /// Generations to evolve (1-10)
        #[arg(long, default_value_t = 3)]
        gens: u32,
        /// Candidates kept per generation (2-8)
        #[arg(long, default_value_t = 4)]
        pop: usize,
    },
}

#[derive(Subcommand, Debug)]
pub enum PluginsCmd {
    /// List catalog + install state
    List,
    /// Install a catalog plugin by id
    Install {
        /// Plugin id (e.g. superpowers, vercel, firecrawl)
        id: String,
    },
    /// Enable an installed plugin
    Enable { id: String },
    /// Disable an installed plugin (keeps files on disk)
    Disable { id: String },
    /// Remove plugin files + registry entry
    Uninstall { id: String },
}

#[derive(Subcommand, Debug)]
pub enum BrowserCmd {
    /// Stage the extension + open your default browser's extensions page
    Setup,
    /// Show detected default browser + extension staging state
    Status,
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
    /// Save API key to ~/.nur/auth.json
    Login {
        /// API key (optional; prompts if omitted)
        #[arg(long)]
        key: Option<String>,
    },
    /// Show auth status (never prints full key)
    Status,
    /// Remove saved key / OAuth session
    Logout {
        /// Best-effort remote revoke note (vendor CLI / account UI); always deletes local file
        #[arg(long)]
        revoke: bool,
    },
}
