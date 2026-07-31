---
name: excalidraw
description: "Hand-drawn Excalidraw diagrams (.excalidraw). Use for architecture, flowcharts, decision trees. Prefer the excalidraw tool over bash."
---

# Excalidraw — diagrams from element JSON

Nur auto-installs `excalidraw-cli` (npm) when Node is available.
Prefer the **`excalidraw`** tool — do not shell out to bash for diagrams.

## When to use

- Architecture diagrams, request/data flows, decision flowcharts
- Not for pixel-perfect UI mockups (use design skills / HTML instead)

## Fast path

1. Build element JSON (camera → dark bg optional → shapes → arrows with bindings)
2. **`excalidraw(action=create, elements=[…], output="docs/foo.excalidraw")`**
   - Writes the file, uploads to excalidraw.com, and **opens the share URL in the user's browser** (default)
   - Do **not** stop at "here's a link" — create already opens it. Tell the user the browser should have opened.
3. Format help: `excalidraw(action=reference)` if stuck on schema
4. `open=false` only when the user asked not to open anything

## Open policy (critical)

- **Excalidraw = web share only.** `open=true` opens the browser to `https://excalidraw.com/...`.
- **Never** OS-open a local `.excalidraw` file:
  - forbidden: `Start-Process path\to\file.excalidraw`
  - forbidden: `open_uri::open_path(.excalidraw)` / `xdg-open` / `open` on the file
  - that triggers Windows **"Open with"** when no association exists
- If export fails or no share URL is printed: leave the file on disk and report the path + error. Do **not** "console open" the local file as a fallback.
- Prefer the `excalidraw` tool. If you must shell to CLI: `excalidraw export <file>` then open **only** the printed `https://excalidraw.com/...` URL.
- For interactive offline boards the user can keep editing natively: use **`tldraw`** / `/draw` (desktop app) — not Excalidraw.

## CLI defaults (omit these in JSON)

roughness=2 · roundness rounded · fontFamily=1 (handwritten) · strokeWidth=2

## Minimal shapes

```json
{ "type": "rectangle", "id": "r1", "x": 100, "y": 100, "width": 200, "height": 80,
  "backgroundColor": "#1e3a5f", "fillStyle": "solid", "strokeColor": "#4a9eed",
  "label": { "text": "Label", "strokeColor": "#e5e5e5" } }
```

Types: `rectangle` · `ellipse` · `diamond` · `text` · `arrow`

## Labels

Use `label: { "text": "…", "fontSize": 20, "strokeColor": "#e5e5e5" }` on shapes and arrows.
CLI expands labels into bound text elements.

## Arrows + bindings

```json
{ "type": "arrow", "id": "a1", "x": 300, "y": 140, "width": 150, "height": 0,
  "points": [[0,0],[150,0]], "endArrowhead": "arrow", "strokeColor": "#4a9eed",
  "startBinding": { "elementId": "b1", "fixedPoint": [1, 0.5] },
  "endBinding": { "elementId": "b2", "fixedPoint": [0, 0.5] },
  "label": { "text": "edge", "strokeColor": "#a0a0a0" } }
```

fixedPoint: right `[1,0.5]` · left `[0,0.5]` · top `[0.5,0]` · bottom `[0.5,1]`

Also add the arrow id to each shape's `boundElements`.

## Camera (4:3 required)

```json
{ "type": "cameraUpdate", "width": 1200, "height": 900, "x": 0, "y": 0 }
```

Sizes: 400×300 S · 600×450 M · **800×600 L** · **1200×900 XL** · 1600×1200 XXL

## Dark mode background (first element after camera)

```json
{ "type": "rectangle", "id": "darkbg", "x": -4000, "y": -3000, "width": 10000, "height": 7500,
  "backgroundColor": "#1e1e2e", "fillStyle": "solid", "strokeColor": "transparent", "strokeWidth": 0 }
```

Dark fills: `#1e3a5f` blue · `#1a4d2e` green · `#2d1b69` purple · `#5c3d1a` amber · `#5c1a1a` red · `#1a4d4d` teal  
Text: `#e5e5e5` primary · `#a0a0a0` muted

## Drawing order

camera → background zones → shape → its text/label → its arrows → next shape…  
**Bad:** all rects then all arrows. **Good:** progressive per node.

## Sizing rules

- Min labeled shape ~120×60 · gaps 20–30px · fontSize 28 title · 20 labels · 14 min
- No emoji (Excalifont does not render them)

## 3-box flow template

```json
[
  { "type": "cameraUpdate", "width": 1200, "height": 900, "x": 0, "y": 100 },
  { "type": "rectangle", "id": "darkbg", "x": -4000, "y": -3000, "width": 10000, "height": 7500,
    "backgroundColor": "#1e1e2e", "fillStyle": "solid", "strokeColor": "transparent", "strokeWidth": 0 },
  { "type": "rectangle", "id": "b1", "x": 60, "y": 350, "width": 220, "height": 90,
    "backgroundColor": "#1e3a5f", "fillStyle": "solid", "strokeColor": "#4a9eed",
    "label": { "text": "Request", "strokeColor": "#e5e5e5" },
    "boundElements": [{ "id": "a1", "type": "arrow" }] },
  { "type": "arrow", "id": "a1", "x": 280, "y": 395, "width": 200, "height": 0,
    "points": [[0,0],[200,0]], "endArrowhead": "arrow", "strokeColor": "#4a9eed",
    "startBinding": { "elementId": "b1", "fixedPoint": [1, 0.5] },
    "endBinding": { "elementId": "b2", "fixedPoint": [0, 0.5] },
    "label": { "text": "process", "strokeColor": "#a0a0a0" } },
  { "type": "rectangle", "id": "b2", "x": 500, "y": 350, "width": 220, "height": 90,
    "backgroundColor": "#5c3d1a", "fillStyle": "solid", "strokeColor": "#f59e0b",
    "label": { "text": "Server", "strokeColor": "#e5e5e5" },
    "boundElements": [{ "id": "a1", "type": "arrow" }, { "id": "a2", "type": "arrow" }] },
  { "type": "arrow", "id": "a2", "x": 720, "y": 395, "width": 200, "height": 0,
    "points": [[0,0],[200,0]], "endArrowhead": "arrow", "strokeColor": "#22c55e",
    "startBinding": { "elementId": "b2", "fixedPoint": [1, 0.5] },
    "endBinding": { "elementId": "b3", "fixedPoint": [0, 0.5] },
    "label": { "text": "respond", "strokeColor": "#a0a0a0" } },
  { "type": "rectangle", "id": "b3", "x": 940, "y": 350, "width": 220, "height": 90,
    "backgroundColor": "#1a4d2e", "fillStyle": "solid", "strokeColor": "#22c55e",
    "label": { "text": "Response", "strokeColor": "#e5e5e5" },
    "boundElements": [{ "id": "a2", "type": "arrow" }] }
]
```

Upstream: https://github.com/ahmadawais/excalidraw-cli
