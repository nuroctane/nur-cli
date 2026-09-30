# Cloudflare Workflows

Use Workflows for durable, multi-step jobs that must retry, wait, and resume without losing completed work. An instance is one execution; steps define persistence and retry boundaries.

Fetch the relevant current documentation before implementing. API shapes, configuration, testing helpers, limits, and examples belong in the docs rather than in this reference.

- **Start a project:** [Build your first Workflow](https://developers.cloudflare.com/workflows/get-started/guide/) covers scaffolding, configuration, deployment, and a first instance.
- **Design durable execution:** [Rules of Workflows](https://developers.cloudflare.com/workflows/build/rules-of-workflows/) covers step boundaries, replay, state, and idempotency.
- **Implement or manage an instance:** [Workers API](https://developers.cloudflare.com/workflows/build/workers-api/) covers steps, instance operations, parameters, and return types.
- **Check capacity and cost:** fetch [limits](https://developers.cloudflare.com/workflows/reference/limits/) and [pricing](https://developers.cloudflare.com/workflows/reference/pricing/) for the target plan.

## In This Reference

- [configuration.md](./configuration.md) — setup, bindings, retry configuration, and local development
- [api.md](./api.md) — steps, instance lifecycle, events, CLI, and REST operations
- [patterns.md](./patterns.md) — design decisions, examples, orchestration, and tests
- [gotchas.md](./gotchas.md) — failures, timeouts, replay, and capacity investigation

## See Also

- [Durable Objects](https://developers.cloudflare.com/durable-objects/) — stateful coordination
- [Queues](../queues/README.md) — asynchronous message delivery
- [Workers](https://developers.cloudflare.com/workers/) — application entry points that trigger instances

---

## License

**GNU General Public License v3.0 (or later)** — see [LICENSE](./LICENSE).

Meta CLI is free software: you may redistribute it and/or modify it under the
terms of the GPL as published by the Free Software Foundation, either version 3
of the License, or (at your option) any later version. It is distributed in the
hope that it will be useful, but **without any warranty**; without even the
implied warranty of merchantability or fitness for a particular purpose.
