---
name: terminal-browser
description: >-
  In-terminal Chromium via terminal-browser (https://terminal-browser.com/).
  Show a website or local HTML beside the agent, then drive the open tab with
  snapshot/click/fill. On Windows, Nur uses a host fallback through
  agent-browser-cli when the upstream binary is missing. Use when the user
  wants a side-by-side browser, HTML plan preview, localhost preview, or
  agent-driven browser actions in the terminal.
---

# terminal-browser

Prefer the **`terminal_browser`** tool (slash `/tb` · `/terminal-browser`). Do not
ask the user to curl-install unless doctor says the runtime is missing.

## Runtimes Nur picks automatically

1. **Native** `terminal-browser` binary (macOS Apple Silicon today; Linux WIP)
2. **WSL** - `wsl -e terminal-browser` when installed inside WSL
3. **Windows-host** - same tool API mapped onto **agent-browser-cli** + real Chrome
   (run `nur browser setup` once for the extension)

## Workflow

1. `terminal_browser` action=`open` url=`localhost:3000` split=`right`
   - Local HTML works: write `plan.html`, then open that path
2. `action`=`ls` to list browsers/tabs (host mode → Chrome tabs)
3. Drive the page:
   - `action`=`action` command=`snapshot`
   - `action`=`action` command=`click @e14`
   - `action`=`action` command=`fill @e3 hello`
   - Or `args`: `["fill", "@e3", "hello"]`

## Notes

- Distinct from the **`browser`** tool name (same Chrome bridge is only the
  Windows-host backend). Prefer `terminal_browser` for the side-by-side /
  HTML-preview workflow the user asked for.
- Upstream in-terminal kitty-graphics Chromium needs a supported terminal
  (Ghostty, kitty, WezTerm, …). Host mode still opens and automates Chrome.
- Install upstream (when on Apple Silicon):  
  `curl -fsSL https://terminal-browser.sh/install | bash`
