# Cloudflare Durable Objects Storage

Use SQLite for new classes. Existing KV-backed classes need their matching API reference; using key-value methods does not by itself identify the backend.

Fetch the relevant current documentation before implementing or reviewing changes.

| Task | Documentation |
|------|---------------|
| Choose SQL, key-value access, transactions, or recovery APIs | [SQLite storage API](https://developers.cloudflare.com/durable-objects/api/sqlite-storage-api/); [Legacy KV storage API](https://developers.cloudflare.com/durable-objects/api/legacy-kv-storage-api/) |
| Configure the backend, class lifecycle, and placement | [Configuration](configuration.md) |
| Find operation semantics and storage options | [API routing](api.md) |
| Design schemas, caches, scheduled work, or cleanup | [Patterns](patterns.md) |
| Diagnose concurrency, limits, and billing | [Troubleshooting](gotchas.md) |
| Verify storage behavior in the Workers runtime | [Testing](testing.md) |

For object routing, WebSockets, and coordination design, see the [Durable Objects skill](../../../durable-objects/SKILL.md).

---

## License

**GNU General Public License v3.0 (or later)** — see [LICENSE](./LICENSE).

Meta CLI is free software: you may redistribute it and/or modify it under the
terms of the GPL as published by the Free Software Foundation, either version 3
of the License, or (at your option) any later version. It is distributed in the
hope that it will be useful, but **without any warranty**; without even the
implied warranty of merchantability or fitness for a particular purpose.
