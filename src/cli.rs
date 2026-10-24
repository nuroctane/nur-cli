use clap::{Parser, Subcommand, ValueEnum};

// Re-export for main.rs match arms.

/// Shape of `nur run` stdout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// The streamed answer, as before.
    Text,
    /// Nothing until the end, then one `{"type":"result", ...}` object.
    Json,
    /// One JSON object per line (`text`, `tool_start`, `tool_end`, `status`,
    /// `layers`), ending in the same `result` object.
    #[value(name = "stream-json")]
    StreamJson,
}

#[derive(Parser, Debug)]
#[command(
    name = "nur",
    version,
    about = "NurCLI — multi-provider coding agent · vision · TUI · tools · 1,000+ skills",
    long_about = "NurCLI — fully loaded multi-provider coding agent.\n\n\
What you get:\n\
  · Streaming Nur-gold TUI — duration chips, expandable thought/tool cards,\n\
    click-to-peek, drag-select + scrollbar, Ctrl+A/C/V, sessions browser\n\
  · Vision — look (images/short video) · extract_frames (ffmpeg keyframes)\n\
  · Real agent harness — manual/plan/auto modes, tools, subagents, todos\n\
  · Ecosystem — Graphify · PLUR · Ruflo · Executor · 1,000+ skills · AKM\n\
  · Hardened by default — sandbox, denylist, SSRF blocks, atomic ~/.nur IO\n\n\
Providers: OpenAI, Anthropic, xAI, Gemini, Meta Model API, OpenRouter, local Ollama, …\n\
Secrets stay in ~/.nur/ only.\n\
Repo: github.com/nuroctane/nur-cli  ·  Invoke as: nur",
    after_long_help = "Environment:\n\
  NUR_HOME        nur's state directory (default ~/.nur). Its parent also becomes\n\
                  the home nur reads ~/.agents, ~/.claude, ~/.codex ... from, so\n\
                  NUR_HOME=<dir>/.nur gives a fully contained install. HOME and\n\
                  USERPROFILE are honoured on every OS; the OS profile is the fallback.\n\
  NUR_MODEL, NUR_PROVIDER   defaults for --model / provider (ignored in child sessions)\n\
\n\
Exit codes (nur run and other headless commands):\n\
  0 ok · 1 unexpected error · 2 configuration · 3 authentication · 4 provider\n\
  5 budget or turn limit · 6 the model declared BLOCKED · 130 cancelled"
)]
pub struct Cli {
    /// Initial prompt for interactive session
    #[arg(value_name = "PROMPT")]
    pub prompt: Option<String>,

    /// Model id (default from config / provider). Env: NUR_MODEL — read in
    /// `main`, not by clap: nur exports NUR_MODEL to its own children for the
    /// ADE hook, and an inherited value must not re-route a child session.
    #[arg(short, long)]
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

    /// Reasoning effort: low|medium|high|xhigh|max|ultracode (also minimal; med, extra, ultra)
    #[arg(long)]
    pub effort: Option<String>,

    /// Max agent turns per prompt (`0` = unlimited; default from config is unlimited)
    #[arg(long)]
    pub max_turns: Option<u32>,

    /// Continuous/sovereign mode: run headless turns toward the prompt as a goal,
    /// looping until the model replies DONE, Ctrl+C, or --max-iters. Mutating tools
    /// ask for approval unless --yes (or --mode auto) is passed.
    /// Example: nur "keep the tests green" --continuous --yes
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

// Parsed once at startup and matched once, so the size gap the lint warns
// about costs nothing; boxing clap fields would only add noise.
#[allow(clippy::large_enum_variant)]
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
        /// What stdout carries: the answer as text, one JSON result object, or
        /// one JSON event per line ending in that result object. Diagnostics
        /// go to stderr in the JSON formats.
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output_format: OutputFormat,
    },
    /// Authentication (API key / status / logout)
    Auth {
        #[command(subcommand)]
        action: AuthCmd,
    },
    /// Show last known token usage (status / usage log paths)
    Usage,
    /// Aggregate local Nur, Claude, Codex, Droid, Pi, Devin, and Command Code usage
    Ledger {
        #[arg(long, default_value="month", value_parser=["today","7","month","all"])]
        period: String,
        #[arg(long)]
        json: bool,
        /// Explicit home directory to read instead of this user's home
        #[arg(long)]
        home: Option<String>,
        /// Download public API prices; reports otherwise run offline
        #[arg(long)]
        refresh_prices: bool,
    },
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
    Update {
        /// Dry run: print local version, latest release, the asset picked for this
        /// platform, and the decision — then exit without installing anything.
        #[arg(long)]
        check: bool,
    },
    /// Diagnose install, auth, config, and ecosystem readiness
    Doctor,
    /// Graphify · PLUR · Ruflo ecosystem (auto-provisioned on open)
    Ecosystem {
        #[command(subcommand)]
        action: EcosystemCmd,
    },
    /// Jev model library, isolated credentials, hosted endpoints and local runtimes
    Jev {
        #[command(subcommand)]
        action: JevCmd,
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
        /// The one chat the bot answers (else $TELEGRAM_CHAT_ID). Unset: pairing only, nothing runs
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
    /// Allow/deny/ask rules: show them, or trust this project's allow rules
    Permissions {
        #[command(subcommand)]
        action: Option<PermissionsCmd>,
    },
    /// Project-local skills: trust or hold this directory's skills
    Skills {
        #[command(subcommand)]
        action: SkillsCmd,
    },
}

/// A project's own skills (`./.nur/skills`, `./.claude/skills`,
/// `./.agents/skills`) are inert until trusted; an edit holds them again.
#[derive(Subcommand, Debug)]
pub enum SkillsCmd {
    /// Trust this project's skills exactly as they stand now
    Trust,
    /// Hold this project's skills again (forget the trust)
    Untrust,
}

/// A project's `.nur/permissions.toml` deny and ask rules always apply; its
/// allow rules wait for `trust`.
#[derive(Subcommand, Debug)]
pub enum PermissionsCmd {
    /// Show the rules in force in this directory (default)
    Show,
    /// Apply this project's allow rules, as they are now
    Trust,
    /// Hold this project's allow rules again
    Untrust,
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
    /// Record a task: `nur bench add <name> "<prompt>" [--check "<cmd>"]`
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

/// Local System One engines (`nur jev …`).
///
/// One bridge serves three open decision engines behind the same typed contract
/// nur already speaks, so the whole harness boost works with no cloud key:
/// `verdict` (openJev-verdict-2.0), `nimble` (Bespoke-Nimble-9B), `laya` (Laya
/// Core ML, Apple Silicon only), plus `mock` for tests and demos.
#[allow(clippy::large_enum_variant)] // parsed once; see `Commands`
#[derive(Subcommand, Debug, Clone)]
pub enum JevCmd {
    /// Search the full pinned JevBench roster
    Models {
        #[arg(long)]
        search: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show an engine's interface, source, hardware and connection steps
    Info { engine: String },
    /// Save an engine-specific key using hidden input
    Login { engine: String },
    /// Show the endpoint, credential source, bridge state, and which backends this machine can run
    Status,
    /// Start a local engine (writes ~/.nur/jev/bridge.json)
    Start {
        /// JevBench engine id (nur jev models)
        #[arg(long)]
        engine: Option<String>,
        /// Model id or local weights path for an adapter
        #[arg(long)]
        model: Option<String>,
        /// Engine API URL; HTTPS or loopback HTTP
        #[arg(long)]
        upstream: Option<String>,
        #[arg(long, value_parser=["systemone","systemone-list","systemone-rune","openai","djev","run","classifier-dev","choice","jevact"])]
        protocol: Option<String>,
        /// Credential environment variable name (never the key value)
        #[arg(long)]
        key_env: Option<String>,
        /// Installed Python adapter module:Class
        #[arg(long)]
        adapter: Option<String>,
        #[arg(long)]
        adapter_dir: Option<String>,
        /// Non-secret constructor or request options as JSON
        #[arg(long)]
        adapter_options: Option<String>,
        /// Engine to serve
        #[arg(long, default_value = "verdict")]
        backend: String,
        /// Loopback port
        #[arg(long, default_value_t = 8788)]
        port: u16,
        /// openJev/GLiClass model id or local checkout (`verdict`)
        #[arg(long)]
        verdict_model: Option<String>,
        /// Directory holding the Nimble weights, if not ./nimble-model (`nimble`)
        #[arg(long)]
        nimble_dir: Option<String>,
        /// Core ML bundle to load (`laya`)
        #[arg(long)]
        laya_model: Option<String>,
        /// Request-token capacity of the laya bundle (`laya`; default 96, the ANE
        /// bundle's total - 128/256/512/1024 for the FluidInference buckets)
        #[arg(long)]
        laya_max_tokens: Option<u64>,
        /// Device for the `verdict` backend
        #[arg(long)]
        device: Option<String>,
    },
    /// Stop the bridge started by `nur jev start`
    Stop,
    /// Point [typesafe] at the local bridge (needs no key); `--hosted` reverts
    Use {
        /// Native TypeSafe-compatible engine id
        #[arg(long)]
        engine: Option<String>,
        /// Native System One URL (HTTPS or loopback HTTP)
        #[arg(long)]
        url: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        key_env: Option<String>,
        #[arg(long, default_value_t = 8788)]
        port: u16,
        /// Go back to the hosted TypeSafe endpoint instead
        #[arg(long, conflicts_with_all=["engine","url","model","key_env"])]
        hosted: bool,
    },
    /// Run the bridge's own mapping selftest (no model, no downloads)
    Selftest,
    /// List the backends this machine can run
    Probe,
    /// Replay a recorded judgment set through the current layer and report
    /// agreement (tune wording/thresholds on dev, confirm on a reserved set)
    Eval {
        /// Recorded set under ~/.nur/jev/evals (record with NUR_JEV_RECORD=<set>)
        #[arg(long)]
        set: String,
        /// Held-out set replayed the same way after the dev set
        #[arg(long)]
        reserved: Option<String>,
        /// Replay at most this many records (quick smoke run)
        #[arg(long)]
        limit: Option<usize>,
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
    /// Install this version's first-party skill pack and rebuild global skill
    /// metadata, without installing packages or resolving credentials
    RefreshSkills,
    /// Install one component (penecho, sgrep, web3-skills, …): prints the pinned
    /// plan first, and only installs with --yes
    Install {
        /// Component, skill pack, or plugin name
        name: String,
        /// Actually install (without it, only the plan is shown)
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum AuthCmd {
    /// Sign in (API key, or a fresh browser / harness sign-in)
    #[command(after_help = crate::providers::login_help_text())]
    Login {
        /// API key (optional; prompts if omitted)
        #[arg(long)]
        key: Option<String>,
        /// Catalog provider id or alias (chatgpt, codex, claude, opencode-go, …). Default: current config.
        #[arg(long, short = 'p')]
        provider: Option<String>,
        /// Fresh browser / device-code / harness sign-in. Never copies another CLI's session.
        #[arg(long)]
        browser: bool,
        /// Same as `nur auth import --from <provider>`: copy an existing vendor CLI / OMP session
        #[arg(long)]
        import: bool,
        /// Model to select after signing in (skips the model question)
        #[arg(long)]
        model: Option<String>,
        /// Accept the client-identity notice and keep the default model without asking
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Copy a vendor CLI's finished sign-in (Codex, Claude Code, Cursor, OpenCode, gcloud, …) into nur
    Import {
        /// Source CLI or provider: codex, claude, antigravity, gcloud, grok, kimi, cursor, opencode, cline, cmd, qwen, droid, gh, az, omp:<provider>, …
        #[arg(long)]
        from: String,
        /// Model to select after importing (skips the model question)
        #[arg(long)]
        model: Option<String>,
        /// Accept the shared-refresh-token warning and keep the default model without asking
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Show auth status (never prints full key)
    Status,
    /// Remove saved key / OAuth session
    Logout {
        /// Best-effort remote revoke note (vendor CLI / account UI); always deletes local file
        #[arg(long)]
        revoke: bool,
    },
    /// Extra API keys a provider rotates through when one is rate-limited, out of quota or refused
    Pool {
        #[command(subcommand)]
        action: PoolCmd,
    },
}

#[derive(Subcommand, Debug)]
pub enum PoolCmd {
    /// Add a key to a provider's rotation (the main key stays as it is)
    Add {
        /// Catalog provider id or alias. Default: current config.
        #[arg(long, short = 'p')]
        provider: Option<String>,
        /// The extra API key
        #[arg(long)]
        key: String,
    },
    /// Show how many keys each provider rotates through (never prints keys)
    List,
    /// Remove a provider's extra keys (the main key stays)
    Clear {
        /// Catalog provider id or alias. Default: current config.
        #[arg(long, short = 'p')]
        provider: Option<String>,
    },
}
