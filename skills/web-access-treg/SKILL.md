---
name: web-access-treg
description: Reach the live web through treg when nur's own web_fetch or web_search falls short - blocked or JavaScript-only pages, site maps, typed extraction, news, papers, and platform data (X, LinkedIn, Reddit, YouTube, stores, reviews, maps). Use when a page will not load, or when the task needs structured data from a platform.
---

# Web access through treg

treg (treg.to, github.com/superdesigndev/treg) routes one token to many web vendors: free
search and page reads first, then the next vendor when one comes back empty or blocked.

## Built into nur

Set `TREG_TOKEN` (a team or agent token from `treg login` or treg.to/app) and nur uses treg
on its own, with no extra steps:

- `web_search` asks `treg.web.search` first and falls back to DuckDuckGo.
- `web_fetch` hands a page that refused the request (401, 403, 429, 5xx) or only renders with
  JavaScript (x.com and most single-page apps) to `treg.web.extract`, and returns markdown.
- Every call prefers the free vendor and carries a 2-cent ceiling per call.

Without a token, nothing is sent to treg.

## Beyond search and page reads

For anything else, use the `treg` CLI through `bash` (check `treg --version`; to install, read
https://treg.to/llms.txt and follow it, which needs the user to sign in once):

| You need | Do this |
| --- | --- |
| Typed fields from a page | `treg catalog search "structured extract"`, then call it with a `schema` or `instruction` |
| Every URL on a site | `treg catalog get treg.web.map` for its parameters (it can rank URLs by a `q` topic), then read only the pages that matter |
| Recent, dated news | `treg catalog search "news search"` |
| A post, profile, video, product, review or place | `treg catalog search "<platform> <object>"` (e.g. "x search posts", "amazon product detail"); never scrape the platform's HTML |

Rules that keep it cheap and correct:

1. Search the catalog by the job in plain words, never trust a remembered endpoint id, and
   prefer routed `treg.*` jobs.
2. Make one small call first and read `_treg.served_by`, `_treg.tried` and the
   `X-Treg-Cost-Micro` header before scaling up.
3. Platform data costs cents per call: tell the user the expected total before a batch, and
   always send `X-Treg-Route-Max-Cost` on batches.
4. A 4xx from the provider (other than 429) means the request is wrong: fix it, do not retry
   it elsewhere. After a 429 or 5xx, try the next provider of the job and say which.
5. Keep the source URL next to every fact you report.
6. Public data only: never log in as the user, use their cookies, get around a paywall, or
   bulk-collect personal data about private individuals.

The full upstream operating guide is https://treg.to/skill.md and the web-access skill is
https://github.com/superdesigndev/treg/blob/main/skills/web-access/SKILL.md; read them when a
call fails in a way this page does not cover.
