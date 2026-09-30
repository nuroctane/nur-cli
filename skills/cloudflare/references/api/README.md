# Cloudflare API Integration

Guide for working with Cloudflare's REST API - authentication, SDK usage, common patterns, and troubleshooting.

## Quick Decision Tree

```
How are you calling the Cloudflare API?
├─ From Workers runtime → Use bindings, not REST API (see ../bindings/)
├─ Server-side (Node/Python/Go) → Official SDK (see api.md)
├─ CLI/scripts → Wrangler or curl (see configuration.md)
├─ Infrastructure-as-code → See ../pulumi/ or ../terraform/
└─ One-off requests → curl examples (see api.md)
```

## SDK Selection

| Language | Package | Best For | Default Retries |
|----------|---------|----------|-----------------|
| TypeScript | `cloudflare` | Node.js, Bun, Next.js, Workers | 2 |
| Python | `cloudflare` | FastAPI, Django, scripts | 2 |
| Go | `cloudflare-go/v4` | CLI tools, microservices | 10 |

All SDKs are Stainless-generated from OpenAPI spec (consistent APIs).

## Authentication Methods

| Method | Security | Use Case | Scope |
|--------|----------|----------|-------|
| **API Token** ✓ | Scoped, rotatable | Production | Per-zone or account |
| API Key + Email | Full account access | Legacy only | Everything |
| User Service Key | Limited | Origin CA certs only | Origin CA |

**Always use API tokens** for new projects.

## Rate Limits

| Limit | Value |
|-------|-------|
| Per user/token | 1200 requests / 5 minutes |
| Per IP | 200 requests / second |
| GraphQL | 320 / 5 minutes (cost-based) |

## Reading Order

| Task | Files to Read |
|------|---------------|
| Initialize SDK client | api.md |
| Configure auth/timeout/retry | configuration.md |
| Find usage patterns | patterns.md |
| Debug errors/rate limits | gotchas.md |
| Product-specific APIs | [Workers docs](https://developers.cloudflare.com/workers/), ../r2/, ../kv/, etc. |

## In This Reference

- **[api.md](api.md)** - SDK client initialization, pagination, error handling, examples
- **[configuration.md](configuration.md)** - Environment variables, SDK config, Wrangler setup
- **[patterns.md](patterns.md)** - Real-world patterns, batch operations, workflows
- **[gotchas.md](gotchas.md)** - Rate limits, SDK-specific issues, troubleshooting

## See Also

- [Cloudflare API Docs](https://developers.cloudflare.com/api/)
- [Bindings Reference](../bindings/) - Workers runtime bindings (preferred over REST API)
- [Wrangler Reference](https://developers.cloudflare.com/workers/wrangler/) - CLI tool for Cloudflare development
- [GraphQL Analytics API Reference](../graphql-api/) - Analytics data via GraphQL (separate endpoint from REST API)

---

## License

**GNU General Public License v3.0 (or later)** — see [LICENSE](./LICENSE).

Meta CLI is free software: you may redistribute it and/or modify it under the
terms of the GPL as published by the Free Software Foundation, either version 3
of the License, or (at your option) any later version. It is distributed in the
hope that it will be useful, but **without any warranty**; without even the
implied warranty of merchantability or fitness for a particular purpose.
