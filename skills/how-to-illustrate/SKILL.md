---
name: how-to-illustrate
description: "MANDATORY router for ANY request to draw, illustrate, diagram, chart, graph, visualize, or map something. Comprehensive diagram-type taxonomy (90+ types) + tool selection (excalidraw/tldraw/penecho) + interactivity/animation mandate. Triggers on every drawing/illustration/graph request, not just explicit tool mentions."
---

# How to Illustrate

**Trigger condition — read this first:** the moment a user asks for ANY drawing,
illustration, diagram, chart, graph, visualization, infographic, or map of
anything — in any phrasing ("draw a diagram of X", "illustrate this", "make a
graph explaining Y", "show me a chart of Z", "visualize this system", "map out
this process") — this skill activates. This does **not** cover the CLI's own
runtime `/sidegraph` (that is internal execution visualization, not user content).

Do not silently default to a markdown table or a wall of prose when a real
diagram/chart is what the user is actually asking for. If you catch yourself
about to describe something spatial, relational, hierarchical, or
sequential in text, stop and check this skill first.

## Step 1 — Pick the right diagram type

Match the user's actual need to a category below, then a specific type. Prefer
the most standard/recognizable type for the domain — don't invent a novel
layout when a standard one exists.

### Processes & Workflows
- **Flowchart** — step-by-step operational workflow, decisions, branching logic
- **Swimlane Diagram** — process steps split into lanes by department/system/actor
- **Sequence Diagram** — chronological message exchanges between components/actors
- **State Machine Diagram** — finite states + triggers/transitions
- **Value Stream Map** — flow of info/tasks/materials from origin to delivery

### Structures & Hierarchies
- **Organizational Chart** — reporting structure / management chains
- **Tree Diagram** — hierarchical classification, parent-child relationships
- **Mind Map** — radial ideas/sub-topics around one central concept
- **Concept Map** — labeled directional arcs between distinct ideas
- **Taxonomy / Pyramid Diagram** — layered classification, broad base to narrow peak

### Relationships & Logic
- **Venn Diagram** — overlapping sets, intersections
- **Euler Diagram** — set inclusion, omitting empty intersections
- **2x2 Matrix / Quadrant Chart** — four quadrants on two axes (e.g. Effort vs Impact)
- **Network Graph (Node-Link)** — interconnected entities and their ties
- **Entity-Relationship Diagram (ERD)** — structural dependencies + cardinalities

### Time, Journey & Narratives
- **Timeline** — milestones/events in linear chronological order
- **Customer Journey Map** — touchpoints, actions, pain points, emotions over phases
- **Storyboard** — scenario/UX workflow as a series of visual panels
- **Gantt Chart (phase view)** — project phases, dependencies, timelines

### Systems & Strategy
- **Causal Loop Diagram** — feedback loops, circular cause-and-effect
- **Decision Tree** — sequential choices/conditions → outcomes
- **SWOT Matrix** — Strengths / Weaknesses / Opportunities / Threats
- **Architecture Diagram** — structural design, component boundaries, interactions

### Biological & Chemical Sciences
- **Cladogram / Phylogenetic Tree** — evolutionary relationships, lineage
- **Metabolic / Signaling Pathway Diagram** — protein/gene/reaction sequences
- **Punnett Square** — genotype/phenotype inheritance grid
- **Ideogram** — chromosome schematic (size, centromere, banding)
- **Skeletal Formula** — minimalist organic-molecule structure
- **Lewis Structure** — covalent bonding + lone pairs
- **Protein Contact Map** — 2D residue-proximity grid

### Physics & Mathematics
- **Feynman Diagram** — subatomic particle interactions
- **Penrose Diagram (Carter-Penrose)** — spacetime causal structure/horizons
- **Jablonski Diagram** — molecular electronic states + transitions
- **Hasse Diagram** — partially ordered sets (posets)
- **Karnaugh Map (K-Map)** — Boolean expression simplification grid
- **Minkowski Diagram** — spacetime graph, light cones, world lines

### Computing, Software & Systems Engineering
- **UML Diagrams** — structural (Class/Object/Component/Deployment) + behavioral (Use Case/Activity/Sequence)
- **Entity-Relationship Diagram (ERD)** — database entity connections
- **Petri Net** — distributed/concurrent/async system modeling
- **Fault Tree Analysis (FTA)** — top-down Boolean-gate failure analysis
- **Logic Gate Diagram** — AND/OR/NOT/XOR circuit schematics
- **Architecture Diagram** — software/hardware structural design
- **Data Flow Diagram (DFD)** — information flow through a system

### Linguistics, Humanities & Social Sciences
- **Parse Tree (Syntax Tree)** — sentence structure per formal grammar
- **Stemma Codicum** — manuscript-version family tree (textual criticism)
- **Isogloss Map** — geographic boundary of a linguistic feature
- **Sociogram** — social links / interpersonal network
- **Kinship Diagram** — descent/marriage/family relationships
- **Argument Map** — premises, objections, conclusions

### Knowledge Representation & Information Architecture
- **Semantic Network** — semantic relations between concepts (AI/NLP)
- **Ontology Graph** — concepts, categories, properties, relationships
- **Concept Lattice** — Formal Concept Analysis hierarchy
- **Topic Map** — ISO topics/associations/occurrences
- **Mind Map** — see above

### Business, Strategy & Process Operations
- **Service Blueprint** — journey map + backstage ops + support processes
- **Wardley Map** — value chain vs evolutionary lifecycle
- **Fishbone Diagram (Ishikawa)** — root-cause categories
- **BPMN Diagram** — standardized business-process flowchart
- **Decision Tree** — see above
- **PERT Chart** — sequence, dependencies, critical path

### Comparison & Ranking
- **Bar Chart (Horizontal)** — long category labels
- **Column Chart (Vertical Bar)** — discrete categories / time intervals
- **Grouped Bar Chart** — subgroups side-by-side
- **Stacked Bar Chart** — subgroup breakdown + total
- **Bullet Graph** — metric vs target/benchmark ranges
- **Radar Chart** — multi-variable data on radiating axes

### Trends Over Time
- **Line Graph** — trend/velocity/trajectory
- **Multi-Line Graph** — multiple series over time
- **Area Chart** — cumulative volume under a line
- **Stacked Area Chart** — total + component contribution over time
- **Candlestick Chart** — OHLC financial price movement
- **Sparkline** — minimalist inline trend, no axes

### Part-to-Whole & Composition
- **Pie Chart** — 2-6 categories summing to 100%
- **Donut Chart** — pie with hollow center for a KPI
- **100% Stacked Bar / Area Chart** — relative percentage contribution
- **Treemap** — nested rectangles, hierarchical part-to-whole
- **Waterfall Chart** — sequential +/- values to a net total

### Distribution & Range
- **Histogram** — frequency density over binned intervals
- **Box Plot** — five-number summary + outliers
- **Violin Plot** — box plot + kernel density shape
- **Density Plot** — smoothed probability distribution curve
- **Dot Plot** — individual raw values on one axis

### Relationships & Correlation
- **Scatter Plot** — paired numerical variables, correlation/outliers
- **Bubble Chart** — scatter plot + third metric as point size
- **Heatmap** — value intensity across a 2D grid via color
- **Parallel Coordinates Plot** — high-dimensional data on parallel axes
- **Connected Scatter Plot** — sequential line through scatter points over time

### Flow, Process & Spatial
- **Sankey Diagram** — flow magnitude between nodes (link width = volume)
- **Funnel Chart** — sequential stage conversion/drop-off
- **Choropleth Map** — geographic regions colored by metric
- **Bubble Map** — scaled circles on geographic coordinates

## Step 2 — Pick the tool

| Need | Tool | Why |
|---|---|---|
| Publishable, shareable, hand-drawn look (docs/PRs/architecture) | **excalidraw** | Browser share URL, versionable `.excalidraw` |
| Editable offline board, live agent-driven canvas, multi-shape interactive layouts | **tldraw** (`/draw`) | Desktop app, document scripts, Editor API |
| Handwriting/math/plots/animated explainer, AI-in-the-loop refinement | **penecho** (`/pen`, `/drawings`, `/penecho`) | Ink canvas, MathJax, declarative animation scenes |
| Quick static reference / no tool available | Inline ASCII, markdown table, or Mermaid-in-code-block | Last resort only |

See `skill(action=read, name=diagram)` for the full routing mechanics and exact
tool-call shapes.

## Step 3 — Interactivity and animation mandate (non-negotiable)

**If the chosen tool supports any element of interactivity or animation, you
MUST use it whenever even remotely useful. Do not neglect these elements.**
A static wall of boxes when the tool could click-to-reveal, animate a flow, or
bind an interactive control is a worse deliverable, not a safer one.

- **tldraw**: prefer real `arrow` bindings over decorative lines; use `frame`
  for grouped sections; when the board explains a process, state machine,
  decision tree, or option comparison, write a `script=`/`script_path=`
  document script so shapes respond to clicks (highlight active path, reveal
  detail panels, run a decision wizard). See `skill(action=read, name=diagram)`
  for the exact `create` args (`script=`, reserved UI band, `fit_camera`).
- **penecho**: when the content has motion, sequence, or before/after states
  (a data pipeline, a process over time, a physical mechanism), use a
  **declarative animation scene** (`animate_scene`, ≤32 objects/motions) instead
  of a single static frame. Use the AI refine / draft layer to iterate live
  rather than redrawing from scratch.
- **excalidraw**: no runtime interactivity in the file itself, but still bind
  arrows to shapes (`startBinding`/`endBinding`) so the diagram stays coherent
  if reordered, and prefer `boundElements` labels over floating text.

**When to skip interactivity:** a genuinely trivial 2-3 box diagram, a static
chart export for a document, or when the user explicitly asked for "just a
static image". Otherwise, default to using whatever live/interactive/animated
capability the chosen tool exposes.

## Step 4 — Execute

1. Pick type (Step 1) + tool (Step 2).
2. Build the diagram using the tool's own skill for exact syntax:
   `skill(action=read, name=excalidraw)` / `name=tldraw-offline` / `name=penecho` / `name=diagram`.
3. Apply the interactivity/animation mandate (Step 3) — do not skip this check.
4. Actually create + open it. Never stop at "here's what I'd draw" — the tools
   open the result for the user automatically; use them.
