# Support Topics

Support topics are small, candidate-scoped investigation guardrails injected into sub-agent briefs.

They are not recommendations, gates, scanners, or broad documentation. A topic tells the investigator what evidence to check, what false positives to avoid, and when to abstain for one class of candidate.

## Add A Topic

Add one file: `references/support-topics/<id>.md`.

The filename must match the `id`. Frontmatter uses a strict subset of YAML: one `key: value` per line, arrays as JSON arrays.

```md
---
id: cdn-cache-auth-safety
title: CDN cache auth safety
status: active
candidateKinds: ["uncached_route", "cache_header_gap"]
frameworks: ["*"]
priority: 90
citations: ["https://vercel.com/docs/caching/cdn-cache"]
maxBriefChars: 900
---

## Investigation Brief
...

## Evidence To Check
...

## Do Not Recommend When
...

## Verification
...
```

## Rules

- Every active topic must cite only URLs or skill-rule refs already present in `references/docs-library.json`.
- Use `candidateKinds` to keep the topic narrow. Use `"*"` only for workflow/protocol topics that truly apply to every candidate.
- Use optional `metrics` only when a topic applies to a specific candidate metric, such as `["LCP"]`, `["INP"]`, or `["CLS"]` for Core Web Vitals.
- Use optional `routePatterns` as JavaScript regex source strings when a topic should appear only for specific candidate routes, such as `["(^|/)404$"]`.
- Keep the body below `maxBriefChars`; the brief renderer caps selected topics before they reach the sub-agent.
- Put URLs in frontmatter only. Topic bodies should describe checks and guardrails, not cite new sources.
- Do not include internal repository paths, service names, pricing tables, exact savings claims, or framework APIs without version gating.

---

## License

**GNU General Public License v3.0 (or later)** — see [LICENSE](./LICENSE).

Meta CLI is free software: you may redistribute it and/or modify it under the
terms of the GPL as published by the Free Software Foundation, either version 3
of the License, or (at your option) any later version. It is distributed in the
hope that it will be useful, but **without any warranty**; without even the
implied warranty of merchantability or fitness for a particular purpose.
