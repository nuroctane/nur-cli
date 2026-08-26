# axiom-alerting

Unified Axiom alerting skill for managing monitors and notifiers via the Axiom v2 API.

## What This Skill Covers

- Monitor lifecycle: list, get, history, create, update, delete
- Notifier lifecycle: list, get, create, update, delete
- End-to-end workflow: create notifier, wire `notifierIds` into monitor, validate behavior

## Requirements

- `curl`
- `jq`
- `~/.axiom.toml` with at least one deployment

Example config:

```toml
[deployments.prod]
url = "https://api.axiom.co"
token = "xaat-your-token"
org_id = "your-org-id"
```

## Setup

```bash
skills/axiom-alerting/scripts/setup
```

## Quick Start

```bash
# List notifiers and monitors
skills/axiom-alerting/scripts/notifier-list prod
skills/axiom-alerting/scripts/monitor-list prod
```

## Common Commands

```bash
# Create notifier from JSON
skills/axiom-alerting/scripts/notifier-create prod ./notifier.json

# Create monitor from JSON
skills/axiom-alerting/scripts/monitor-create prod ./monitor.json

# Check monitor history in a time range
skills/axiom-alerting/scripts/monitor-history prod <monitor-id> 2026-05-03T00:00:00Z 2026-05-04T00:00:00Z
```

## JSON Notes

- Email notifier uses `emails`, not `recipients`.
- Monitor payload uses `notifierIds` to attach destinations.
- For noisy alerts, prefer `triggerAfterNPositiveResults` with `triggerFromNRuns`.

## Script Index

- `scripts/axiom-api <deploy> <method> <path> [body]`
- `scripts/monitor-list <deployment> [--json]`
- `scripts/monitor-get <deployment> <id>`
- `scripts/monitor-history <deployment> <id> <startTime> <endTime>`
- `scripts/monitor-create <deployment> <json-file>`
- `scripts/monitor-update <deployment> <id> <json-file>`
- `scripts/monitor-delete <deployment> <id>`
- `scripts/notifier-list <deployment> [--json]`
- `scripts/notifier-get <deployment> <id>`
- `scripts/notifier-create <deployment> <json-file>`
- `scripts/notifier-update <deployment> <id> <json-file>`
- `scripts/notifier-delete <deployment> <id>`

---

## License

**GNU General Public License v3.0 (or later)** — see [LICENSE](./LICENSE).

Meta CLI is free software: you may redistribute it and/or modify it under the
terms of the GPL as published by the Free Software Foundation, either version 3
of the License, or (at your option) any later version. It is distributed in the
hope that it will be useful, but **without any warranty**; without even the
implied warranty of merchantability or fitness for a particular purpose.
