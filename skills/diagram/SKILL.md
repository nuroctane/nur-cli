---
name: diagram
description: "Router for Excalidraw (publishable hand-drawn), tldraw offline (interactive desktop boards), and PenEcho (AI ink/math canvas). Use when the user wants a diagram, architecture chart, whiteboard, or canvas."
---

# Diagram router

Pick **one** primary product. Never conflate open policies.

| Intent | Tool | Open policy |
|---|---|---|
| Architecture, flowchart, decision tree, PR/docs diagram | **excalidraw** | Browser **share URL only** — never OS-open `.excalidraw` |
| Offline interactive board, agent scripts, live edits | **tldraw** / `/draw` | Desktop app on `.tldraw` |
| Handwriting, MathJax, plots, AI refine, animations | **penecho** / `/pen` | Browser `http://127.0.0.1:3888` |

## Fast paths

### Excalidraw (publish)
```
excalidraw(action=create, elements=[…], output="docs/arch.excalidraw")
# or from mermaid:
excalidraw(action=create, from_mermaid="flowchart LR\n  A[In] --> B[Out]", name="flow")
```
- Default dir when `output` omitted: `.nur/diagrams/<name>.excalidraw`
- Never Desktop (reserved for tldraw)
- `open=true` → browser share URL only

### tldraw (offline board)
```
tldraw(action=create, title="Board", shapes=[
  {x:80,y:80,w:200,h:100,text:"A",color:"blue",type:"geo"},
  {x:360,y:80,w:200,h:100,text:"B",type:"geo"},
  {x:280,y:120,w:80,h:0,type:"arrow",text:"next"}
])
tldraw(action=screenshot)  # → .nur/media then look
tldraw(action=list_docs)
```

### PenEcho (AI canvas)
```
penecho(action=launch)                    # ensure + config + browser
penecho(action=inject, inject="…context") # seed + open
penecho(action=stop|restart, port=3888)
```
Long installs: `background=true` or `bg(action=run, …)`.

## Background jobs
Long work must not block the agent turn:
- `bg(action=run, command="…", label="…")` → returns job id
- Status chip in TUI footer; `/bg` · `/bg <id>` · `/bg cancel <id>`
- `tldraw(action=install, background=true)` · `penecho(…, background=true)`

## Forbidden
- `Start-Process` / `open_path` on `.excalidraw`
- Inventing `.tldraw` JSON with `write_file`
- Asking the user which canvas product when the intent is clear
