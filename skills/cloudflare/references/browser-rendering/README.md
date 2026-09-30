# Browser Run (formerly Browser Rendering)

Use Browser Run for screenshots, PDFs, rendered content extraction, and browser automation. Read the relevant current documentation before implementing; use the [documentation index](https://developers.cloudflare.com/browser-run/llms.txt) to discover additional guides.

Choose the integration by the work and runtime:

- For a self-contained screenshot, PDF, or extraction, start with Quick Actions. They are available through REST and Workers bindings; check the chosen action's supported interface.
- For multi-step interactions or persistent state, use browser sessions. In Workers, use Cloudflare's Puppeteer or Playwright package; from external scripts or CI, use the CDP integration.
- When adapting existing automation, preserve its library where supported and check installed versions against the corresponding guide.

Read only the reference needed for the task:

| Task | Reference |
|------|-----------|
| Set up bindings, dependencies, or development | [configuration.md](configuration.md) |
| Select an endpoint or browser client API | [api.md](api.md) |
| Implement a workflow or manage reusable sessions | [patterns.md](patterns.md) |
| Diagnose failures or plan capacity and cost | [gotchas.md](gotchas.md) |

---

## License

**GNU General Public License v3.0 (or later)** — see [LICENSE](./LICENSE).

Meta CLI is free software: you may redistribute it and/or modify it under the
terms of the GPL as published by the Free Software Foundation, either version 3
of the License, or (at your option) any later version. It is distributed in the
hope that it will be useful, but **without any warranty**; without even the
implied warranty of merchantability or fitness for a particular purpose.
