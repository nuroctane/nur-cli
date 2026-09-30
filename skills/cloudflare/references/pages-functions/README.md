# Cloudflare Pages Functions

Use this reference for server-side behavior in an existing Pages project. For new applications, follow the Workers recommendation in the [Pages framework guidance](https://developers.cloudflare.com/pages/framework-guides/).

| Task | Documentation |
| --- | --- |
| Identify filesystem routes and invocation boundaries | [Routing](https://developers.cloudflare.com/pages/functions/routing/) |
| Implement request handlers | [API reference](https://developers.cloudflare.com/pages/functions/api-reference/) |
| Understand generated Worker output | [Advanced mode](https://developers.cloudflare.com/pages/functions/advanced-mode/) |

Inspect whether the project uses a Functions directory or framework-generated advanced mode before selecting a routing approach. Fetch current documentation for signatures, supported bindings, configuration, and examples.

## In This Reference

- [api.md](./api.md) — handlers, context, middleware, and assets
- [configuration.md](./configuration.md) — bindings, environments, types, and local development
- [patterns.md](./patterns.md) — request ownership and shared logic
- [gotchas.md](./gotchas.md) — route, binding, and runtime investigation

See [Pages](../pages/README.md) for builds and deployment decisions.

---

## License

**GNU General Public License v3.0 (or later)** — see [LICENSE](./LICENSE).

Meta CLI is free software: you may redistribute it and/or modify it under the
terms of the GPL as published by the Free Software Foundation, either version 3
of the License, or (at your option) any later version. It is distributed in the
hope that it will be useful, but **without any warranty**; without even the
implied warranty of merchantability or fitness for a particular purpose.
