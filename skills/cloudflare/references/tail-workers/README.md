# Cloudflare Tail Workers

Use Tail Workers when execution events need custom processing. Fetch the current documentation before implementing handlers, configuration, or integrations.

| Task | Documentation |
| --- | --- |
| Decide whether custom processing is needed | [Tail Workers](https://developers.cloudflare.com/workers/observability/logs/tail-workers/) |
| Export logs and traces to an observability destination | [Exporting OpenTelemetry Data](https://developers.cloudflare.com/workers/observability/exporting-opentelemetry-data/) |
| Inspect a deployment interactively | [Real-time logs](https://developers.cloudflare.com/workers/observability/logs/real-time-logs/) |
| Implement the consumer | [Tail handler](https://developers.cloudflare.com/workers/runtime-apis/handlers/tail/) |

Before adding a Tail Worker, check whether built-in OpenTelemetry export meets the destination’s needs. Use the Tail Workers guide for the tradeoff, then identify the custom filtering or transformation that remains necessary.

## In This Reference

- [configuration.md](./configuration.md) — producer, consumer, destination, and environment setup
- [api.md](./api.md) — event fields, execution outcomes, and redaction
- [patterns.md](./patterns.md) — destination and filtering decisions
- [gotchas.md](./gotchas.md) — connection, data, and delivery investigation

See [observability](../observability/README.md) for broader logging and tracing choices.

---

## License

**GNU General Public License v3.0 (or later)** — see [LICENSE](./LICENSE).

Meta CLI is free software: you may redistribute it and/or modify it under the
terms of the GPL as published by the Free Software Foundation, either version 3
of the License, or (at your option) any later version. It is distributed in the
hope that it will be useful, but **without any warranty**; without even the
implied warranty of merchantability or fitness for a particular purpose.
