# Cloudflare Queues

Use Queues to decouple producers from asynchronous consumers and buffer bursts of work. Design consumers for duplicate delivery; use Workflows when the task needs durable multi-step orchestration.

Fetch the relevant documentation below before implementing. Treat current Cloudflare docs as the source of truth for API signatures, acknowledgement semantics, configuration, limits, and pricing.

## Choose a consumer

- Use a Worker push consumer when processing runs on Workers.
- Use an HTTP pull consumer when processing runs in another environment; plan for polling, visibility timeouts, and acknowledgement leases.
- Choose a message encoding the consumer can decode. Check serialization and compatibility-date behavior before sending existing application objects.

See [How Queues works](https://developers.cloudflare.com/queues/reference/how-queues-works/) and [delivery guarantees](https://developers.cloudflare.com/queues/reference/delivery-guarantees/) before choosing ordering or deduplication strategies.

## Read by task

| Task | Reference |
|------|-----------|
| Create queues, bind producers, and configure consumers | [configuration.md](./configuration.md) |
| Send messages and implement acknowledgement or retries | [api.md](./api.md) |
| Buffer APIs, defer jobs, or integrate with storage and orchestration | [patterns.md](./patterns.md) |
| Diagnose delivery failures, duplicates, or capacity issues | [gotchas.md](./gotchas.md) |

For a first application, fetch [Getting started](https://developers.cloudflare.com/queues/get-started/). Retrieve [limits](https://developers.cloudflare.com/queues/platform/limits/) and [pricing](https://developers.cloudflare.com/queues/platform/pricing/) before sizing throughput, retention, or cost; plan-specific values are not maintained here.

---

## License

**GNU General Public License v3.0 (or later)** — see [LICENSE](./LICENSE).

Meta CLI is free software: you may redistribute it and/or modify it under the
terms of the GPL as published by the Free Software Foundation, either version 3
of the License, or (at your option) any later version. It is distributed in the
hope that it will be useful, but **without any warranty**; without even the
implied warranty of merchantability or fitness for a particular purpose.
