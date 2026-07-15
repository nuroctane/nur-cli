//! Built-in plugin marketplace catalog (same set as official xAI index + Nur extras).

/// A marketplace entry. Installed via git into `~/.nur/plugins/<id>/`.
#[derive(Debug, Clone, Copy)]
pub struct PluginEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub category: &'static str,
    /// Git clone URL (https).
    pub source_url: &'static str,
    /// Optional subdirectory inside the repo that is the plugin root.
    pub path_in_repo: Option<&'static str>,
}

/// Full catalog shown in `/plugins`.
pub const CATALOG: &[PluginEntry] = &[
    PluginEntry {
        id: "superpowers",
        name: "Superpowers",
        description: "TDD, systematic debugging, collaboration patterns, engineering workflows",
        category: "development",
        source_url: "https://github.com/obra/superpowers.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "vercel",
        name: "Vercel",
        description: "Deploy, env vars, Next.js, AI SDK, Marketplace — Vercel platform skills",
        category: "deployment",
        source_url: "https://github.com/vercel/vercel-plugin.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "chrome-devtools",
        name: "Chrome DevTools",
        description: "Live Chrome control: network, console, performance traces, automation",
        category: "development",
        source_url: "https://github.com/ChromeDevTools/chrome-devtools-mcp.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "firecrawl",
        name: "Firecrawl",
        description: "Scrape, crawl, and search the web into clean LLM-ready markdown",
        category: "development",
        source_url: "https://github.com/firecrawl/firecrawl-grok-plugin.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "figma",
        name: "Figma",
        description: "Design-to-code: read Figma context, Code Connect, canvas write",
        category: "development",
        source_url: "https://github.com/figma/mcp-server-guide.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "sentry",
        name: "Sentry",
        description: "Error monitoring: issues, stack traces, production debug",
        category: "monitoring",
        source_url: "https://github.com/getsentry/plugin-grok.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "cloudflare",
        name: "Cloudflare",
        description: "Workers, Durable Objects, Wrangler, MCP servers, web performance",
        category: "development",
        source_url: "https://github.com/cloudflare/skills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "mongodb",
        name: "MongoDB",
        description: "Database explore, collections, queries, Atlas best practices",
        category: "database",
        source_url: "https://github.com/mongodb/agent-skills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "axiom",
        name: "Axiom",
        description: "Logs/metrics with APL, SRE investigations, monitors, cost analysis",
        category: "observability",
        source_url: "https://github.com/axiomhq/skills.git",
        path_in_repo: None,
    },
    PluginEntry {
        id: "railway",
        name: "Railway",
        description: "Deploy services, DBs, env vars, domains, metrics on Railway",
        category: "deployment",
        source_url: "https://github.com/railwayapp/railway-skills.git",
        path_in_repo: Some("plugins/railway"),
    },
    PluginEntry {
        id: "fable",
        name: "Fable",
        description: "Think / act / prove workflow: fable-method, fable-loop, fable-judge",
        category: "development",
        source_url: "https://github.com/Sahir619/fable-method.git",
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
