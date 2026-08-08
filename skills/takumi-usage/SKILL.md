---
name: takumi-usage
description: Render OG cards and images from JSX/HTML with Takumi (kane50613) — the no-headless-browser Rust engine. For when a project or task wants image/OG-card output without spinning up Chromium.
disable-model-invocation: false
---

# Takumi usage (kane50613) — render images from markup, no headless browser

Takumi is a Rust rendering engine that turns JSX / HTML / node trees into PNG,
JPEG, WebP, SVG, GIF, and PDF — no headless Chromium (saves ~300MB RAM per card
vs. a browser). Drop-in compatible with `next/og`. Current: `takumi-js` /
`takumi` = **2.5.11** (npm + crates.io).

> **Nur CLI note:** bundling `takumi` as a first-class binary tool is blocked by
> an upstream `takumi-raster` build issue, so there is no `nur render_card`
> command. But `takumi-js` is a normal npm package you CAN add to any Node
> project (repo, scratch dir, or `nur` scratch) and render cards with — no
> browser, no Rust compile. Use that path (below); revisit the crate when
> upstream fixes takumi-raster.

## Runnable standalone recipe (no browser, no Rust build)

Works in any Node >= 20 project (or a throwaway dir). This is the command the
agent should actually run when a task wants an OG card / share image:

```ts
// render-card.ts
import { render } from "takumi-js";
import { writeFileSync } from "node:fs";

const png = await render(
  <div tw="w-full h-full flex items-center justify-center bg-slate-900">
    <h1 tw="text-6xl font-bold text-white">Hello from Takumi</h1>
    <p tw="text-2xl text-slate-300">rendered without Chromium</p>
  </div>,
  { width: 1200, height: 630 }
);
writeFileSync("card.png", png);
```

```bash
# in the project/scratch dir:
npm i takumi-js           # or: bun i takumi-js
node render-card.ts       # or: bun render-card.ts  → writes card.png
```

Note: `render` returns a `Buffer`/`Uint8Array` — write it with `writeFileSync`
rather than `console.log` (binaries become garbled text if printed to stdout).

## Use it

- In a **Node/JS project**: `bun i takumi-js` then `import { render } from "takumi-js"`.
- Via an **API route** (`next/og`-compatible):
  `import { ImageResponse } from "takumi-js/response"`.
- As a **Rust crate**: `cargo add takumi` (field `cargo add takumi --features from-html,svg-backend`).

## Typical OG card

```tsx
import { ImageResponse } from "takumi-js/response";

export function GET() {
  return new ImageResponse(
    <div tw="w-full h-full flex items-center justify-center bg-gradient-to-b from-blue-100 to-red-50">
      <h1 tw="text-6xl font-bold">Hello from Takumi</h1>
    </div>,
    { width: 1200, height: 630 }
  );
}
```

## Static / SVG / animated from JS

```ts
import { render, renderSvg, renderAnimation } from "takumi-js";

const png = await render(<div tw="w-full h-40 bg-slate-900" />, { width: 1200, height: 630 });
const svg = await renderSvg(<div tw="bg-white"><p>Vector</p></div>, { width: 800, height: 400 });
const webp = await renderAnimation({
  width: 400, height: 400, fps: 30, format: "webp",
  scenes: [{ durationMs: 1000, node: <div tw="... animate-spin">...</div> }],
});
```

## Rust API (already in the crate docs)

```rust
use takumi::prelude::*;
use takumi::{from_html, render};

let node = from_html(r#"<div style="background:red;width:100%;height:100%"></div>"#, Default::default())?;
let options = RenderOptions::builder()
  .viewport(Viewport::new((1200, 630)))
  .node(node)
  .fonts(&Fonts::default())
  .build();
let image = render(options)?;
```

## Key capabilities worth knowing

- **Tailwind v4 utilities** incl. arbitrary values; `tw="..."` attribute.
- **Auto-scaling text**: `text-fit` (`grow`/`shrink`), OpenType `font-variation-settings`.
- **Motion paths**: `offset-path` (`ray()`, `path()`, shapes) for animations.
- **Filters & blend**: `filter` (blur/brightness/drop-shadow/…), `mix-blend-mode`, `backdrop-filter`, `clip-path`.
- **CSS Grid**, block/inline/float, `::before/::after`, `:is()`/`:where()`, `background-clip: text`, conic gradients, RTL.
- **Fonts**: WOFF2/WOFF/TTF via `googleFonts()` or files; register once on a `Renderer` for batch.

## Comparison guardrail

Only reach for Takumi when you need pixel/vector output from markup (OG cards,
share images, dynamic social artifacts). For diagrams (architecture, flow,
decision trees) use the `excalidraw` tool. For offline desktop boards use `tldraw`.
Don't launch a headless browser for a card — that's exactly the churn Takumi avoids.

## Source

Drawn from the takumi repo (kane50613) README + `.agents/skills/takumi-usage/SKILL.md`.
Nur integration status is honest: crate path blocked by an upstream takumi-raster
build bug; CLI/JS paths remain usable.
