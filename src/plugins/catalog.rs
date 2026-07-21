//! Built-in plugin marketplace catalog.
//!
//! Categories are deliberate browse facets for `/plugins` (type a category name
//! to filter). Order in [`CATEGORIES`] is the sort order in the picker.

/// A marketplace entry. Installed via git into `~/.nur/plugins/<id>/`.
#[derive(Debug, Clone, Copy)]
pub struct PluginEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// Browse facet — keep short (shown in the picker). See [`CATEGORIES`].
    pub category: &'static str,
    /// Git clone URL (https).
    pub source_url: &'static str,
    /// Optional subdirectory inside the repo that is the plugin root.
    pub path_in_repo: Option<&'static str>,
}

/// Category order for the `/plugins` picker (unknown categories sort last).
/// Type any of these labels in the filter to focus the list.
pub const CATEGORIES: &[&str] = &[
    "workflow",      // agent methodology / loops
    "engineering",   // production software craft
    "specs",         // SDD / planning / tickets
    "design",        // UI taste, motion, craft
    "design-system", // tokens, FSD, brand extraction
    "browser",       // chrome / scrape / e2e
    "deploy",        // ship platforms
    "cloud",         // vendor product skills
    "data",          // databases
    "observability", // logs / errors / metrics
    "marketing",     // growth / SEO / content
    "finance",       // markets / valuation / CRE
    "trading",       // brokers / live trading APIs
    "crypto",        // web3 / defi / on-chain (when skill-shaped)
    "security",      // red/blue / offensive / hardening
    "science",       // research / journals / labs
    "robotics",      // robot software agents
    "catalog",       // mega packs / awesome indexes
];

/// Full catalog shown in `/plugins`.
pub const CATALOG: &[PluginEntry] = &[
    // ── workflow ──────────────────────────────────────────────────────────
    PluginEntry {
        id: "superpowers",
        name: "Superpowers",
        description: "TDD, systematic debugging, brainstorming, subagent workflows — the default agent methodology",
        category: "workflow",
        source_url: "https://github.com/obra/superpowers.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "fable",
        name: "Fable",
        description: "Think / act / prove: fable-method, fable-loop, fable-judge",
        category: "workflow",
        source_url: "https://github.com/Sahir619/fable-method.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "builderio",
        name: "Builder.io Skills",
        description: "Agent efficiency: efficient-fable, plan-arbiter, stay-within-limits, visual-plan",
        category: "workflow",
        source_url: "https://github.com/BuilderIO/skills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "compound-engineering",
        name: "Compound Engineering",
        description: "Every Inc compound-engineering plugin — multi-step agent workflows teams love",
        category: "workflow",
        source_url: "https://github.com/EveryInc/compound-engineering-plugin.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "gstack",
        name: "gstack",
        description: "Snarktank gstack — stacked agent workflows / AI engineering task graphs",
        category: "workflow",
        source_url: "https://github.com/snarktank/gstack.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "wshobson-agents",
        name: "wshobson Agents",
        description: "Large Claude agent ecosystem — 100+ specialized skills across plugins",
        category: "workflow",
        source_url: "https://github.com/wshobson/agents.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "softaworks",
        name: "Softaworks Agent Toolkit",
        description: "Practical agent toolkit skills for everyday coding-agent sessions",
        category: "workflow",
        source_url: "https://github.com/softaworks/agent-toolkit.git",
        path_in_repo: None,
    },

    // ── engineering ───────────────────────────────────────────────────────
    PluginEntry {
        id: "mattpocock",
        name: "Matt Pocock Skills",
        description: "Real-engineering skills: grill-me, triage, tdd, to-spec, implement, handoff",
        category: "engineering",
        source_url: "https://github.com/mattpocock/skills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "addyosmani",
        name: "Addy Osmani Agent Skills",
        description: "Production engineering: context engineering, frontend UI, security, shipping, TDD",
        category: "engineering",
        source_url: "https://github.com/addyosmani/agent-skills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "anthropic-skills",
        name: "Anthropic Official Skills",
        description: "Official public Agent Skills examples from Anthropic (patterns + inspiration)",
        category: "engineering",
        source_url: "https://github.com/anthropics/skills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "microsoft-skills",
        name: "Microsoft Skills",
        description: "Azure / Foundry / Microsoft AI agent skills, custom agents, MCP configs",
        category: "engineering",
        source_url: "https://github.com/microsoft/skills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "strativd-ai-skills",
        name: "strativd AI Skills",
        description: "Curated SKILL.md collection following the open Agent Skills spec",
        category: "engineering",
        source_url: "https://github.com/strativd/ai-skills.git",
        path_in_repo: None,
    },

    PluginEntry {
        id: "vercel-agent-skills",
        name: "Vercel Agent Skills",
        description: "Vercel's official collection of agent skills for shipping web software",
        category: "engineering",
        source_url: "https://github.com/vercel-labs/agent-skills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "nanocodex",
        name: "nanocodex",
        description: "Minimal Rust coding agent (gakonst) — reference tools incl. a lean web_search",
        category: "engineering",
        source_url: "https://github.com/gakonst/nanocodex.git",
        path_in_repo: None,
    },

    // ── specs ─────────────────────────────────────────────────────────────
    PluginEntry {
        id: "spec-kit",
        name: "GitHub Spec Kit",
        description: "Spec-driven development kit from GitHub — specs before code",
        category: "specs",
        source_url: "https://github.com/github/spec-kit.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "spec-driven-development",
        name: "Spec-Driven Development",
        description: "Claude skill that keeps coding agents on a written spec (FredAntB)",
        category: "specs",
        source_url: "https://github.com/FredAntB/Spec-Driven-Development.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "ai-dev-tasks",
        name: "AI Dev Tasks",
        description: "Snarktank task breakdown patterns for AI coding agents",
        category: "specs",
        source_url: "https://github.com/snarktank/ai-dev-tasks.git",
        path_in_repo: None,
    },

    // ── design ────────────────────────────────────────────────────────────
    PluginEntry {
        id: "impeccable",
        name: "Impeccable",
        description: "Design language for AI harnesses: audit / polish / critique / animate (46 detectors)",
        category: "design",
        source_url: "https://github.com/pbakaus/impeccable.git",
        path_in_repo: Some("plugin"),
    },
    PluginEntry {
        id: "mengto",
        name: "Meng To Skills",
        description: "Design + web craft: UI prompting, motion systems, brand worlds, capture/perf",
        category: "design",
        source_url: "https://github.com/MengTo/Skills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "ui-craft",
        name: "UI Craft",
        description: "Design engineering system for AI coding agents — large craft-oriented skill set",
        category: "design",
        source_url: "https://github.com/educlopez/ui-craft.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "design-skills",
        name: "Design Skills (ihlamury)",
        description: "Opinionated UI constraints extracted from the best product UIs",
        category: "design",
        source_url: "https://github.com/ihlamury/design-skills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "oh-my-design",
        name: "Oh My Design",
        description: "Give your AI coding agent a design system — one-command design skill pack",
        category: "design",
        source_url: "https://github.com/kwakseongjae/oh-my-design.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "taste-skill",
        name: "Taste Skill",
        description: "Stops AI UI slop — taste constraints beloved by frontend engineers (Leonxlnx)",
        category: "design",
        source_url: "https://github.com/Leonxlnx/taste-skill.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "superdesign",
        name: "Superdesign Skill",
        description: "Design skill for Claude Code, Cursor, and other agents (superdesigndev)",
        category: "design",
        source_url: "https://github.com/superdesigndev/superdesign-skill.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "anydesign",
        name: "Anydesign",
        description: "Analyze images, websites, and Figma into design guidance for agents",
        category: "design",
        source_url: "https://github.com/uxKero/anydesign.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "ui-ux-pro-max",
        name: "UI/UX Pro Max",
        description: "High-bar UI/UX skill pack for product-quality interfaces",
        category: "design",
        source_url: "https://github.com/nextlevelbuilder/ui-ux-pro-max-skill.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "awesome-design-skills",
        name: "Awesome Design Skills",
        description: "Curated index of DESIGN.md + SKILL.md design skills (bergside)",
        category: "design",
        source_url: "https://github.com/bergside/awesome-design-skills.git",
        path_in_repo: None,
    },
    // ── recent.design curated design-engineer skills ──────────────────────
    PluginEntry {
        id: "emil-skills",
        name: "Emil Kowalski Skills",
        description: "Skills for Design Engineers — animation, craft, and UI polish (emilkowalski)",
        category: "design",
        source_url: "https://github.com/emilkowalski/skills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "ui-skills",
        name: "UI Skills (ibelick)",
        description: "Design-engineer skills for building refined, motion-aware interfaces",
        category: "design",
        source_url: "https://github.com/ibelick/ui-skills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "make-interfaces-feel-better",
        name: "Make Interfaces Feel Better",
        description: "Skill that makes your interfaces feel better — micro-interaction taste (jakubkrehel)",
        category: "design",
        source_url: "https://github.com/jakubkrehel/make-interfaces-feel-better.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "oklch-skill",
        name: "OKLCH Color Skill",
        description: "Work with OKLCH colors — perceptual palettes and accessible contrast (jakubkrehel)",
        category: "design",
        source_url: "https://github.com/jakubkrehel/oklch-skill.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "userinterface-wiki",
        name: "User Interface Wiki",
        description: "A living manual for better interfaces — UI patterns and principles (raphaelsalaja)",
        category: "design",
        source_url: "https://github.com/raphaelsalaja/userinterface-wiki.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "dialkit",
        name: "Dialkit",
        description: "Dial in interface parameters of any kind — tune motion/layout/token knobs (joshpuckett)",
        category: "design",
        source_url: "https://github.com/joshpuckett/dialkit.git",
        path_in_repo: None,
    },

    // ── design-system ─────────────────────────────────────────────────────
    PluginEntry {
        id: "extract-design-system",
        name: "Extract Design System",
        description: "Pull colors, typography, spacing tokens from a live site or brand",
        category: "design-system",
        source_url: "https://github.com/arvindrk/extract-design-system.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "brand-to-design-md",
        name: "Brand to Design.md",
        description: "Turn a public brand URL into a portable design.md skill",
        category: "design-system",
        source_url: "https://github.com/shaom/brand-to-design-md-skill.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "feature-sliced",
        name: "Feature-Sliced Design Skills",
        description: "Agent skills for applying Feature-Sliced Design architecture",
        category: "design-system",
        source_url: "https://github.com/feature-sliced/skills.git",
        path_in_repo: None,
    },

    // ── browser ───────────────────────────────────────────────────────────
    PluginEntry {
        id: "agent-browser",
        name: "Agent Browser",
        description: "Browser automation CLI for AI agents — navigate, act, and extract (vercel-labs)",
        category: "browser",
        source_url: "https://github.com/vercel-labs/agent-browser.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "chrome-devtools",
        name: "Chrome DevTools",
        description: "Live Chrome control: network, console, performance traces, automation",
        category: "browser",
        source_url: "https://github.com/ChromeDevTools/chrome-devtools-mcp.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "firecrawl",
        name: "Firecrawl",
        description: "Scrape, crawl, and search the web into clean LLM-ready markdown",
        category: "browser",
        source_url: "https://github.com/firecrawl/firecrawl-grok-plugin.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "playwright-skill",
        name: "Playwright Skill",
        description: "Browser e2e automation skill for Claude/Cursor agents (lackeyjb)",
        category: "browser",
        source_url: "https://github.com/lackeyjb/playwright-skill.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "figma",
        name: "Figma",
        description: "Design-to-code: read Figma context, Code Connect, canvas write",
        category: "browser",
        source_url: "https://github.com/figma/mcp-server-guide.git",
        path_in_repo: None,
    },

    // ── deploy ────────────────────────────────────────────────────────────
    PluginEntry {
        id: "vercel",
        name: "Vercel",
        description: "Deploy, env vars, Next.js, AI SDK, Marketplace — Vercel platform skills",
        category: "deploy",
        source_url: "https://github.com/vercel/vercel-plugin.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "railway",
        name: "Railway",
        description: "Deploy services, DBs, env vars, domains, metrics on Railway",
        category: "deploy",
        source_url: "https://github.com/railwayapp/railway-skills.git",
        path_in_repo: Some("plugins/railway"),
    },

    // ── cloud ─────────────────────────────────────────────────────────────
    PluginEntry {
        id: "cloudflare",
        name: "Cloudflare",
        description: "Workers, Durable Objects, Wrangler, MCP servers, web performance",
        category: "cloud",
        source_url: "https://github.com/cloudflare/skills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "google-skills",
        name: "Google Skills",
        description: "Official Google product skills (Ads, Analytics, Cloud, Firebase, Gemini, …)",
        category: "cloud",
        source_url: "https://github.com/google/skills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "nvidia-skills",
        name: "NVIDIA Skills",
        description: "CUDA, cuOpt, accelerated computing, AIQ research/deploy (300 skill packs)",
        category: "cloud",
        source_url: "https://github.com/NVIDIA/skills.git",
        path_in_repo: None,
    },

    // ── data ──────────────────────────────────────────────────────────────
    PluginEntry {
        id: "mongodb",
        name: "MongoDB",
        description: "Database explore, collections, queries, Atlas best practices",
        category: "data",
        source_url: "https://github.com/mongodb/agent-skills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "langextract",
        name: "LangExtract",
        description: "Extract structured info from unstructured text via LLMs with source grounding (google)",
        category: "data",
        source_url: "https://github.com/google/langextract.git",
        path_in_repo: None,
    },

    // ── observability ─────────────────────────────────────────────────────
    PluginEntry {
        id: "axiom",
        name: "Axiom",
        description: "Logs/metrics with APL, SRE investigations, monitors, cost analysis",
        category: "observability",
        source_url: "https://github.com/axiomhq/skills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "sentry",
        name: "Sentry",
        description: "Error monitoring: issues, stack traces, production debug",
        category: "observability",
        source_url: "https://github.com/getsentry/plugin-grok.git",
        path_in_repo: None,
    },

    // ── marketing ─────────────────────────────────────────────────────────
    PluginEntry {
        id: "ai-marketing",
        name: "AI Marketing Skills",
        description: "Growth, SEO ops, content ops, outbound, sales pipeline, clone-site (ericosiu)",
        category: "marketing",
        source_url: "https://github.com/ericosiu/ai-marketing-skills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "marketingskills",
        name: "Marketing Skills (coreyhaines31)",
        description: "Popular marketing skill pack for growth experiments and go-to-market agents",
        category: "marketing",
        source_url: "https://github.com/coreyhaines31/marketingskills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "open-seo",
        name: "OpenSEO",
        description: "Open-source Semrush/Ahrefs alternative — keyword/backlink/rank/audit MCP (every-app)",
        category: "marketing",
        source_url: "https://github.com/every-app/open-seo.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "akarso",
        name: "Akarso",
        description: "Post, schedule, and reply across 14 social platforms from the terminal (remorses)",
        category: "marketing",
        source_url: "https://github.com/remorses/akarso.git",
        path_in_repo: None,
    },

    // ── finance ───────────────────────────────────────────────────────────
    PluginEntry {
        id: "finance-skills",
        name: "Finance Skills",
        description: "Financial analysis: valuation, earnings, options, market data readers (himself65)",
        category: "finance",
        source_url: "https://github.com/himself65/finance-skills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "longbridge",
        name: "Longbridge Skills",
        description: "Markets: portfolio, quant, technicals, earnings, value investing, watchlist",
        category: "finance",
        source_url: "https://github.com/longbridge/skills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "buffett",
        name: "Buffett Skills",
        description: "Value-investing skill pack built on Warren Buffett principles (agi-now)",
        category: "finance",
        source_url: "https://github.com/agi-now/buffett-skills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "cre-skills",
        name: "CRE Agent Skills",
        description: "Commercial real estate: underwriting, due diligence, financing, brokerage",
        category: "finance",
        source_url: "https://github.com/ahacker-1/cre-agent-skills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "cc-finance",
        name: "CC Finance Skills",
        description: "Claude finance helpers: yfinance data, correlations, market utilities",
        category: "finance",
        source_url: "https://github.com/JacobHsu/cc-finance-skills.git",
        path_in_repo: None,
    },

    // ── trading ───────────────────────────────────────────────────────────
    PluginEntry {
        id: "alpaca-skills",
        name: "Alpaca Skills",
        description: "Agent skills for Alpaca Trading API + Broker API (stocks, options, crypto rails)",
        category: "trading",
        source_url: "https://github.com/alpacahq/alpaca-skills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "vibe-trade",
        name: "Vibe Trade",
        description: "AI trading agent skill system — drop a SKILL.md to extend strategy tooling",
        category: "trading",
        source_url: "https://github.com/spyderweb47/Vibe-Trade.git",
        path_in_repo: None,
    },

    // ── crypto (skill-shaped packs; pure frameworks stay out of catalog) ──
    // Note: most DeFi repos are frameworks, not SKILL.md packs. Prefer finance +
    // trading packs that include crypto rails (e.g. Alpaca) until more pure
    // web3 skill packs mature. Claude-Red is offensive security, not on-chain.

    // ── security ──────────────────────────────────────────────────────────
    PluginEntry {
        id: "claude-red",
        name: "Claude Red",
        description: "Curated offensive-security skill library for authorized red-team agent work",
        category: "security",
        source_url: "https://github.com/SnailSploit/Claude-Red.git",
        path_in_repo: None,
    },

    // ── science ───────────────────────────────────────────────────────────
    PluginEntry {
        id: "scientific",
        name: "Scientific Agent Skills",
        description: "K-Dense AI scientist pack: biopython, astropy, benchling, paper search, lab tooling",
        category: "science",
        source_url: "https://github.com/K-Dense-AI/scientific-agent-skills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "journal-skills",
        name: "Awesome Journal Skills",
        description: "Academic journal skill packs (AAAI, ACL, AEJ, …) — huge; paper workflows only",
        category: "science",
        source_url: "https://github.com/brycewang-stanford/Awesome-Journal-Skills.git",
        path_in_repo: None,
    },

    // ── robotics ──────────────────────────────────────────────────────────
    PluginEntry {
        id: "robotics-skills",
        name: "Robotics Agent Skills",
        description: "Skills that make coding agents better at robot software (arpitg1304)",
        category: "robotics",
        source_url: "https://github.com/arpitg1304/robotics-agent-skills.git",
        path_in_repo: None,
    },

    // ── catalog ───────────────────────────────────────────────────────────
    PluginEntry {
        id: "claude-skills-mega",
        name: "Claude Skills Mega Pack",
        description: "Large multi-domain pack (business, agents, growth, ops) — heavy download",
        category: "catalog",
        source_url: "https://github.com/alirezarezvani/claude-skills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "awesome-claude-skills",
        name: "Awesome Claude Skills",
        description: "ComposioHQ curated index of Claude/agent skills across the ecosystem",
        category: "catalog",
        source_url: "https://github.com/ComposioHQ/awesome-claude-skills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "awesome-claude-skills-travis",
        name: "Awesome Claude Skills (travisvn)",
        description: "Community-curated Claude skills list — discovery companion",
        category: "catalog",
        source_url: "https://github.com/travisvn/awesome-claude-skills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "antigravity-skills",
        name: "Antigravity Awesome Skills",
        description: "Large community skill index (sickn33) — browse then install selectively",
        category: "catalog",
        source_url: "https://github.com/sickn33/antigravity-awesome-skills.git",
        path_in_repo: None,
    },
];

pub fn catalog() -> &'static [PluginEntry] {
    CATALOG
}

pub fn by_id(id: &str) -> Option<&'static PluginEntry> {
    let id = id.trim();
    CATALOG.iter().find(|p| p.id.eq_ignore_ascii_case(id))
}

/// Sort key for category browse order (unknown → last).
pub fn category_rank(category: &str) -> usize {
    CATEGORIES
        .iter()
        .position(|c| c.eq_ignore_ascii_case(category))
        .unwrap_or(CATEGORIES.len())
}
