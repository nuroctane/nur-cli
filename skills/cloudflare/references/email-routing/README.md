# Email Routing

Use routing rules for address-based forwarding; use an Email Worker when incoming mail needs custom processing. Fetch the linked docs before implementing APIs, DNS, configuration, or limits.

| Task | Start here |
| --- | --- |
| Forward incoming mail to an existing mailbox | [Route emails](https://developers.cloudflare.com/email-service/get-started/route-emails/) |
| Manage addresses, verification, catch-all rules, or subaddressing | [Routing rules and addresses](https://developers.cloudflare.com/email-service/configuration/email-routing-addresses/) |
| Filter, parse, reply to, or store incoming mail | [Email Workers](../email-workers/README.md) |
| Send a new outbound message | [Send emails](https://developers.cloudflare.com/email-service/get-started/send-emails/) — Workers binding, REST API, or SMTP |

Forwarding requires verified destinations. Replying within an incoming email event and sending a new outbound message have different requirements; use the relevant API docs.

## Reference map

- [Configuration](configuration.md): domains, rules, deployment, and local testing.
- [API](api.md): routing management and inbound/outbound operations.
- [Patterns](patterns.md): filtering, parsing, storage, and notifications.
- [Troubleshooting](gotchas.md): authentication, delivery, and current limits.

---

## License

**GNU General Public License v3.0 (or later)** — see [LICENSE](./LICENSE).

Meta CLI is free software: you may redistribute it and/or modify it under the
terms of the GPL as published by the Free Software Foundation, either version 3
of the License, or (at your option) any later version. It is distributed in the
hope that it will be useful, but **without any warranty**; without even the
implied warranty of merchantability or fitness for a particular purpose.
