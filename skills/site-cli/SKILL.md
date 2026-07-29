---
name: site-cli
description: Derive a fast, reusable API client or CLI for any website by recording its network requests to a HAR file instead of driving the browser every time. Use whenever the user mentions building a "site CLI", making a CLI for a website/service, watching or recording network requests, HAR files, reverse-engineering a site's private/internal API, or automating a site that has no public API. The HAR is the source of truth; a client built without one is guessing.
---

# site-cli

Browser automation is slow and brittle - every action re-drives a real browser.
Instead: **capture the site's network traffic once (a HAR file), read the real
requests it makes, and generate a direct HTTP client.** The derived client hits
the same endpoints the site does, minus the browser. Faster, cheaper, scriptable.

Credit: trick from @jlongster, popularized by @thdxr (built an Uber Eats CLI this way).

## The loop

### 1. Record the HAR

Drive the site through the exact flow you want to automate (log in, search, add
to cart, checkout - whatever the CLI needs to do), while recording all network
traffic to a HAR file.

- **Chrome/Edge/Firefox DevTools** -> Network tab -> do the flow -> right-click ->
  **Save all as HAR with content**.
- **Via the `browser` tool** (preserves the user's login session): open the site,
  perform the flow, then use `browser action=network` to capture requests, or
  `exec` to pull `performance.getEntries()` / dump fetch calls.
- Keep **"Preserve log"** on so navigations don't wipe the capture.
- Do the flow **deliberately and minimally** - one clean pass per capability. Noise
  in the HAR is noise in the client.

### 2. Read the HAR, find the real API

A HAR is JSON: `log.entries[]`, each with `request` (method, url, headers,
postData) and `response` (status, content). Parse it and extract the requests
that actually carry the data - usually XHR/fetch to `/api/...`, `/graphql`, or a
JSON content-type. Ignore images, fonts, analytics, tracking beacons.

For each meaningful call, record:

- **Method + URL** (note path vs query params)
- **Auth**: which header/cookie/token carries the session (`Authorization`,
  `Cookie`, `x-csrf-token`, a signed query param). This is the part that must be
  supplied at runtime.
- **Request body shape** (JSON/form fields) and which fields are user input vs
  fixed vs derived.
- **Response shape** - the fields the CLI cares about.

Quick triage in a shell:

```bash
# list every JSON-ish request in a HAR
jq -r '.log.entries[]
  | select(.response.content.mimeType | test("json"))
  | "\(.request.method) \(.request.url)"' capture.har | sort -u
```

### 3. Derive the client

Generate a small typed client - one function per endpoint - that reproduces the
requests. Prefer the language the surrounding project already uses.

- Parameterize the **inputs** (search term, item id, address) and the **secrets**
  (token/cookie), read from args/env - never hardcode a captured token.
- Keep required headers the server actually checks (auth, content-type, sometimes
  a `user-agent` or `referer` allowlist); drop the rest and see if it still works.
- Add a tiny CLI wrapper (subcommands mapping to the endpoints).

### 4. Verify against reality

- Run each derived call and diff the response against what the HAR captured.
- Confirm the auth story: does the token expire? Is there a refresh/login call in
  the HAR you must replay first? Wire that in.
- Handle the unhappy paths you can see in the HAR (401 -> re-auth, rate limits).

## Rules of thumb

- **The HAR is ground truth.** Do not invent endpoints or parameters - if it's not
  in a captured request, you're guessing. Re-record to cover a missing flow.
- **Auth is the hard part.** Most sites gate on a session cookie or bearer token.
  Find exactly what carries it; that's the one thing the user must provide.
- **Strip aggressively, then test.** Start from the full captured request, remove
  headers/params one class at a time, keep what breaks when removed.
- **Never commit captured secrets.** HAR files contain live cookies and tokens.
  Treat `.har` as sensitive; gitignore it; scrub tokens from examples.
- **Respect ToS and rate limits.** Deriving a private-API client can violate a
  site's terms. Flag this to the user for anything beyond personal automation, and
  don't hammer endpoints.
- **Prefer a public API if one exists.** This technique is for sites that have no
  documented API - check for one first.

## Output

A runnable client/CLI plus a short note: which endpoints it wraps, what auth it
needs and where to get it, and how it was verified against the HAR.
