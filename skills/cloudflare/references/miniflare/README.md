# Miniflare

Miniflare provides programmatic control of local Workers simulation. Read the linked documentation before choosing APIs, configuration, or a migration path.

## Choose the testing tool

| Need | Start here |
|------|------------|
| Unit tests that execute in the Workers runtime | [Workers Vitest setup](https://developers.cloudflare.com/workers/testing/vitest-integration/write-your-first-test/) |
| Integration tests against built Workers | [Integration test harness](https://developers.cloudflare.com/workers/testing/test-harness/) |
| Low-level simulator control for a custom harness | [Miniflare testing guide](https://developers.cloudflare.com/workers/testing/miniflare/writing-tests/) |
| Binding access from a Node.js process | [Wrangler getPlatformProxy](https://developers.cloudflare.com/workers/wrangler/api/#getplatformproxy) |

For interactive local development, use the project's Wrangler or Cloudflare Vite workflow. Direct Miniflare is useful when the higher-level testing tools do not expose the control needed.

## Read for the task

- [Get started](https://developers.cloudflare.com/workers/testing/miniflare/get-started/) — installation, scripts, lifecycle, and event dispatch.
- [API routing](./api.md) — events and access to local resources.
- [Configuration](./configuration.md) — modules, bindings, compatibility, and multiple Workers.
- [Testing patterns](./patterns.md) — runtime choice, mocking, and test lifecycle.
- [Troubleshooting and migrations](./gotchas.md) — build/configuration differences and existing test suites.

---

## License

**GNU General Public License v3.0 (or later)** — see [LICENSE](./LICENSE).

Meta CLI is free software: you may redistribute it and/or modify it under the
terms of the GPL as published by the Free Software Foundation, either version 3
of the License, or (at your option) any later version. It is distributed in the
hope that it will be useful, but **without any warranty**; without even the
implied warranty of merchantability or fitness for a particular purpose.
