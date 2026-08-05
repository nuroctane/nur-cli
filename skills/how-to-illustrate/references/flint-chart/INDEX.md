# Flint Chart (integrated into how-to-illustrate)

Upstream: https://github.com/microsoft/flint-chart

Flint is Microsoft Research's visualization language for AI agents — compact
semantic chart specs (`ChartAssemblyInput`) that compile to Vega-Lite, ECharts,
Chart.js, Plotly, or Excel.

## When to use (from how-to-illustrate Step 2)

Use **flint** whenever the deliverable is a **data chart** (bars, lines, scatter,
heatmap, sankey, funnel, KPI, etc.) rather than a systems/architecture/process
diagram. Prefer Flint MCP (`npx -y flint-chart-mcp`) → `create_chart_view` when
available; otherwise author the Flint spec and assemble via `flint-chart`.

Architecture / flow / decision boards still go to **excalidraw** / **tldraw** /
**penecho**.

## Full authoring skill

Read `flint-chart-author.SKILL.md` in this folder for the complete channels,
semantic types, chartType catalog, MCP tools, and worked examples.

## Install

```bash
npm install flint-chart
npx -y flint-chart-mcp
```
