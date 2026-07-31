---
name: how-to-illustrate
description: "MANDATORY router for ANY request to draw, illustrate, diagram, chart, graph, visualize, or map something. Exhaustive diagram-type taxonomy (30+ categories, 200+ named types, each with what it shows, its structural anatomy, and when to reach for it) + tool selection (excalidraw/tldraw/penecho) + interactivity/animation mandate. Triggers on every drawing/illustration/graph request, not just explicit tool mentions."
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
about to describe something spatial, relational, hierarchical, temporal, or
causal in text, stop and check this skill first. A good diagram answers "where
does this fit / what does this connect to / what happens next" in a glance;
prose forces the reader to reconstruct that structure in their head. If the
content has shape, that shape belongs on the canvas, not in a paragraph.

## Step 1 — Pick the right diagram type

Match the user's actual need to a category below, then a specific type within
it. Each entry gives you: what it depicts, its structural anatomy (the parts
you actually need to draw), and when it's the right call versus a
near-neighbor. Prefer the most standard/recognizable type for the domain —
readers pattern-match on convention, so don't invent a novel layout when an
established one exists and fits. When two types are close, the "vs." note
tells you how to pick.

---

### Processes & Workflows

- **Flowchart** — Step-by-step operational logic: rounded-rect for start/end,
  rectangles for actions, diamonds for yes/no decisions, arrows for flow
  direction. Use whenever a reader needs to *follow* a procedure rather than
  understand its architecture. Keep decision diamonds binary where possible —
  a diamond with four exits is a smell that it should be a lookup table or a
  swimlane instead. Vs. state machine: a flowchart describes a single
  execution's path through steps; a state machine describes the *system's*
  condition over time, revisitable and re-enterable.
- **Swimlane Diagram (Cross-Functional Flowchart)** — A flowchart split into
  horizontal or vertical lanes, one per department/system/actor responsible
  for each step. The lane a box sits in is itself information — use this the
  moment "who does this step" matters as much as "what happens." Handoffs
  between lanes (an arrow crossing a lane boundary) are usually the most
  interesting part of the diagram; make them visually obvious.
- **Sequence Diagram** — Vertical lifelines (one per actor/component/service),
  time flowing top-to-bottom, horizontal arrows for messages between them.
  Purpose-built for showing *order and timing* of interactions — an API call
  chain, a protocol handshake, a multi-service request. Add activation bars
  (thin rectangles on a lifeline) to show when a component is actively
  processing versus idle/waiting.
- **State Machine Diagram (Finite State Machine)** — Nodes are named states
  (rounded shapes), labeled directed edges are triggers/events/transitions.
  Always mark the initial state (filled circle or explicit "start" arrow) and
  any terminal states. Use for anything with discrete modes it can enter,
  stay in, and leave under specific conditions — a UI component's states, an
  order's lifecycle, a game character's behavior.
- **Value Stream Map (VSM)** — Lean-manufacturing-derived flow of
  information/materials/tasks from raw input to customer delivery, annotated
  with lead time, cycle time, and wait time at each stage, plus a
  time-ladder along the bottom separating value-add from non-value-add time.
  Use for process-improvement/efficiency conversations, not general
  documentation flowcharts.
- **Cross-Functional Process Map** — Broader cousin of the swimlane, often
  spanning multiple pages/systems with explicit input/output artifacts drawn
  at each handoff (a document icon, a data object) rather than just an arrow.
- **Pipeline Diagram** — Linear left-to-right stages each transforming an
  input into an output (a build pipeline, a data pipeline, an ETL job). Unlike
  a generic flowchart, branches are rare; the point is throughput and stage
  responsibility, so keep stages uniform in visual weight.
- **Onboarding / Setup Flow** — A specialized flowchart for a first-run or
  account-creation sequence; benefits from screen-thumbnail-style boxes
  (mini wireframes) rather than plain labeled rectangles so it doubles as a
  UX reference, not just logic.

### Structures & Hierarchies

- **Organizational Chart** — Top-down tree of reporting lines; box = person or
  role, line = "reports to." Dotted lines denote a secondary/dotted-line
  report. Keep peer boxes at identical width/height — visual weight implies
  seniority even when unintended.
- **Tree Diagram (Hierarchical Tree)** — General parent-child hierarchy for
  anything with strict single-parent nesting: taxonomies, file systems,
  category trees, tournament brackets' inverse. Vs. mind map: a tree diagram
  is usually top-down/left-right with a fixed root and formal levels; a mind
  map is radial and exploratory.
- **Mind Map** — Central topic in the middle, branches radiating outward to
  sub-topics, further sub-branches beyond that. Optimized for *idea capture*
  and free association, not formal hierarchy — branches can be uneven,
  colors can group themes, and it's fine if it's not perfectly symmetric.
  Use during brainstorming/exploration, not for a finished structural
  reference (use a tree diagram for that).
- **Concept Map** — Nodes are distinct concepts, but unlike a mind map the
  connecting lines are *labeled* with the specific relationship ("causes,"
  "is part of," "requires") and any node can connect to any other, not just
  radiate from one center. Use when the relationships between ideas carry as
  much meaning as the ideas themselves.
- **Taxonomy / Pyramid Diagram** — Layered triangle/pyramid, broad base
  narrowing to a peak, each layer a classification tier (Maslow's hierarchy
  is the canonical example). Use when there's a genuine "more of this = less
  common/more advanced" relationship between layers — don't reach for a
  pyramid just because you have levels; use a tree if the levels don't imply
  volume or foundational dependency.
- **Nested Box Diagram (Containment Diagram)** — Boxes drawn inside boxes to
  show literal containment/scope (a module inside a service inside a system).
  Distinct from a tree: the nesting itself *is* the relationship, no
  connecting lines needed.
- **Bracket / Tournament Diagram** — Binary-tree-shaped elimination structure,
  matches narrowing toward a champion. Single-elimination is a clean binary
  tree; double-elimination needs a mirrored losers' bracket feeding back into
  the same final — don't collapse the two into one tree.
- **Composition / Decomposition Diagram (Work Breakdown Structure)** — Tree
  that decomposes one deliverable into its constituent parts/tasks down to
  actionable leaf nodes. Used in project management to scope work; keep leaf
  nodes at roughly the same granularity across branches.

### Relationships, Sets & Logic

- **Venn Diagram** — Overlapping circles/ellipses, each region a distinct
  logical combination of set membership. Works cleanly up to 3 sets; beyond
  that the overlaps become unreadable — switch to an UpSet plot or a table
  instead of forcing a 4+ circle Venn.
- **Euler Diagram** — Like a Venn but circles only overlap where an actual
  intersection exists (some pairs may not touch at all, one may sit fully
  inside another). Use when set relationships are *not* symmetric — e.g. "all
  cats are mammals" (full containment, no partial overlap) is an Euler
  diagram, not a Venn.
- **2×2 Matrix / Quadrant Chart** — Two perpendicular axes (each a spectrum,
  e.g. Effort vs. Impact, Urgent vs. Important) dividing the plane into four
  labeled quadrants; items are plotted or grouped by quadrant. Use for
  prioritization/positioning conversations where two independent variables
  drive a decision.
- **Network Graph (Node-Link Diagram)** — Freeform nodes and edges with no
  imposed hierarchy; use for peer-to-peer relationships (social networks,
  dependency graphs, citation networks) where "who reports to whom" doesn't
  apply but "who's connected to whom" does. Edge weight/thickness can encode
  relationship strength; node size can encode centrality/importance.
- **Entity-Relationship Diagram (ERD)** — Entities as boxes, relationships as
  labeled connecting lines with cardinality notation (1, many, 0..1, etc.,
  in crow's-foot or UML style) at each end. The definitive diagram for
  database schema design — always show cardinality, it's the whole point.
- **Dependency Graph (DAG)** — Directed, acyclic node-link diagram where an
  edge means "depends on" / "must happen before." Use for build systems,
  task scheduling, or module dependencies. If you find a cycle, that's a bug
  in the underlying system, not just the diagram — flag it.
- **Affinity Diagram** — Loose sticky-note-style clusters of related items
  grouped by theme with no formal connecting lines, themes emerging from
  proximity/color rather than explicit edges. Use for synthesizing
  qualitative research (user interview notes, brainstorm output) into
  categories.
- **Correlation Matrix (as a relationship diagram)** — Grid of all pairwise
  relationships between N variables, cell color/value showing correlation
  strength/direction. Effectively a heatmap specialized for
  variable-to-variable relationships; use before a scatter-plot matrix when
  you have more than ~5 variables to screen at once.

### Time, Journey & Narrative

- **Timeline** — Single axis (usually horizontal), events/milestones plotted
  at their actual or relative date, often with a short label and icon per
  event. Use for chronological history/roadmap content where *when* is the
  primary axis of meaning.
- **Roadmap (Product/Strategy Timeline)** — Timeline variant grouped into
  swim-lanes by team/workstream or banded into quarters/phases (Now/Next/
  Later), each item a bar or card rather than a point. Distinguish from a
  Gantt chart: a roadmap communicates intent/sequencing at a coarse level;
  a Gantt commits to exact durations and dependencies.
- **Customer / User Journey Map** — Horizontal phases of an experience
  (Awareness → Consideration → Purchase → Onboarding → Advocacy, or similar),
  with rows underneath for actions, touchpoints, thoughts, emotions (often
  an emotion curve), and pain points at each phase. Always include the
  emotion curve if the request is even loosely about experience quality —
  it's the row people actually read first.
- **Storyboard** — Sequential panels (comic-strip style), each a snapshot of
  a scene/screen/moment with a short caption, read left-to-right/top-to-
  bottom. Use for UX walkthroughs, video/animation planning, or any scenario
  that unfolds as discrete visual beats rather than continuous flow.
- **Gantt Chart** — Horizontal bars on a time axis, one row per task, bar
  length = duration, position = start/end date, with dependency arrows
  between bars and often a "today" marker line. The tool for committed
  project schedules with real dates and precedence — don't use it for
  loose/exploratory sequencing (use a roadmap or timeline instead).
- **Plot Diagram / Story Arc (Freytag's Pyramid)** — Rising-then-falling line
  across five labeled beats: Exposition → Rising Action → Climax → Falling
  Action → Resolution. Use for narrative structure analysis (a book, film,
  or even a marketing campaign's story).
- **Hero's Journey Circle** — Circular (not linear) narrative-stage diagram,
  stages arranged around a ring (Call to Adventure → Trials → Return, etc.).
  Use specifically when the narrative genuinely returns to its starting
  point/state — that circularity is the entire point of the shape.
- **Historical Timeline with Parallel Tracks** — Multiple horizontal timeline
  tracks stacked vertically (e.g. political / cultural / technological
  history in the same era), letting the reader see what else was happening
  simultaneously. Use when cross-domain simultaneity is the insight, not
  just a single sequence of events.

### Systems, Strategy & Causality

- **Causal Loop Diagram** — Nodes are variables, directed arrows show
  influence, each arrow marked + (same direction) or − (opposite direction);
  closed loops are marked reinforcing (R) or balancing (B). The
  systems-thinking tool for showing feedback rather than one-way cause and
  effect — use whenever "X affects Y which affects X again" is the actual
  claim.
- **Decision Tree** — Root node is the initial decision/question, each
  branch a choice or condition, leaf nodes are outcomes; can be pure logic
  (if/then) or annotated with probabilities and expected values for
  decision-analysis use. Vs. flowchart: a decision tree is specifically
  about *choices and their consequences* fanning outward, not a linear
  procedure with occasional branches.
- **SWOT Matrix** — Fixed 2×2 grid: Strengths, Weaknesses, Opportunities,
  Threats, internal factors on top, external on bottom (or however your
  convention runs) — bullet lists inside each quadrant, no connecting lines
  needed. Purely a structured-brainstorm container, not a flow or hierarchy.
- **Architecture Diagram** — Boxes for components/services/modules, lines
  for their interactions/data flow, grouped into logical tiers or bounded
  contexts (often nested boxes for "this all lives in one VPC/service").
  The single most overloaded term in this list — always clarify: system
  architecture (services + data flow). software architecture (modules +
  dependencies), or physical/infra architecture (servers, networks, zones)
  before drawing, since the right level of detail differs a lot.
- **Fishbone Diagram (Ishikawa / Cause-and-Effect)** — Horizontal spine
  pointing to the effect/problem at the head, diagonal "bones" branching off
  for major cause categories (commonly Methods, Machines, Materials, Manpower,
  Measurement, Environment), each with sub-branches for specific causes. The
  standard root-cause-analysis diagram — use it, not a mind map, when the
  explicit goal is finding what *caused* a specific problem.
- **Force Field Diagram** — Central vertical line (the status quo), arrows
  pushing from the left (driving forces) and right (restraining forces)
  toward or against a change, arrow length/weight indicating relative
  strength. Use for change-management/decision conversations about what's
  helping vs. hindering a specific shift.
- **Stakeholder Map** — Grid or radial plot positioning stakeholders by
  power/interest (or influence/impact), often as a 2×2 with names as dots or
  small labeled circles. A specialized 2×2 matrix; call it a stakeholder map
  when the axes are specifically power and interest.
- **Wardley Map** — Value chain on the vertical axis (user need at top,
  down through the components that serve it), evolution stage (Genesis →
  Custom-Built → Product → Commodity) on the horizontal axis; components are
  dots positioned by both, connected by the dependencies that build the
  chain above them. Purpose-built for strategy conversations about where to
  build vs. buy vs. wait — don't substitute a generic architecture diagram
  when evolutionary positioning is the actual point.

### Computing, Software & Systems Engineering

- **UML Class Diagram** — Boxes divided into three compartments (class name /
  attributes / methods), connected by relationship lines with specific
  arrowheads for inheritance (hollow triangle), composition (filled
  diamond), aggregation (hollow diamond), and association (plain line), plus
  multiplicity labels at each end. Use for object-oriented design docs where
  the exact relationship *kind* matters, not just "these are related."
- **UML Sequence / Use Case / Activity Diagrams** — See Sequence Diagram
  above for the message-timing variant; Use Case diagrams show actors
  (stick figures) connected to oval use-cases they perform, good for
  scoping system boundaries at a requirements level; Activity diagrams are
  UML's flowchart variant with swimlane-like "partitions" and explicit
  fork/join bars for parallel activity.
- **Entity-Relationship Diagram** — see Relationships & Logic above; the
  database-schema application specifically pairs each entity box with its
  key attributes listed inside it.
- **Petri Net** — Places (circles) hold tokens (dots), transitions (bars)
  consume tokens from input places and produce tokens in output places,
  modeling concurrent/asynchronous systems formally. Reach for this over a
  state machine when *multiple simultaneous states/resources* need modeling,
  not a single system in one state at a time.
- **Fault Tree Analysis (FTA)** — Top-down tree from a single top failure
  event, branching downward through intermediate causes connected by AND/OR
  logic gates (standard Boolean-logic gate symbols) to root-cause basic
  events at the leaves. The reliability-engineering counterpart to a
  fishbone diagram — use FTA when causes combine via explicit Boolean logic
  (both X AND Y must fail), fishbone when causes are just categorized.
- **Logic Gate Diagram** — Standard schematic symbols (AND, OR, NOT, XOR,
  NAND, NOR) wired together showing a digital circuit's Boolean logic.
- **Data Flow Diagram (DFD)** — Processes (circles/rounded rects), data
  stores (open rectangles/parallel lines), external entities (squares), and
  labeled arrows for the data moving between them, often leveled (Level 0
  context diagram down to Level 1/2 detail). Use for systems-analysis
  documentation focused specifically on *what data moves where*, distinct
  from an architecture diagram's focus on components.
- **Deployment / Infrastructure Diagram** — Physical/cloud nodes (servers,
  containers, regions, VPCs) as boxes, often using platform-specific icon
  sets (AWS/GCP/Azure shapes), with network/communication lines between
  them. Group by trust boundary or network zone using nested boxes/frames.
- **Component Diagram** — Boxes with the UML "lollipop and socket" notation
  for provided/required interfaces between components; use when interface
  contracts between modules are the point, not just "these talk to each
  other."
- **Git Branching Diagram** — Horizontal/vertical lines per branch, commits
  as dots along each line, merges as converging lines. Purpose-built for
  explaining a branching strategy (trunk-based, gitflow, etc.) — the visual
  convention is extremely standardized, don't improvise the layout.
- **API Request/Response Diagram** — A specialized sequence diagram between
  client and server lifelines, annotated with the actual method/status code
  on each arrow (`GET /users/:id`, `200 OK {…}`); useful for API
  documentation where exact payload shape matters more than general timing.

### Cybersecurity & Networks

- **Network Topology Diagram** — Physical/logical network layout: routers,
  switches, firewalls, servers, endpoints as standard icons, connected by
  lines representing physical/logical links, often grouped into subnets or
  VLANs via nested boxes. Star, bus, ring, and mesh are the canonical
  topologies — name which one you're drawing if it's a teaching diagram.
- **Attack Tree** — Root node is the attacker's goal, branching downward
  through AND/OR-connected sub-goals to concrete attack steps at the leaves
  (structurally identical to a fault tree, applied to adversarial rather
  than accidental failure). Use for threat-modeling conversations.
- **Kill Chain / Attack Path Diagram** — Linear or graph-based sequence of
  attacker stages (Recon → Initial Access → Persistence → Lateral Movement →
  Exfiltration, or similar named framework) mapped onto the specific systems
  involved. Distinct from an attack tree: this shows one realized path
  through time, not the full space of possible attacks.
- **Data Flow Diagram with Trust Boundaries** — Standard DFD (see above) with
  dashed trust-boundary lines added around zones of differing privilege,
  used specifically for threat modeling (STRIDE-style) — every place a flow
  crosses a boundary is a thing to interrogate.

### Biological & Life Sciences

- **Cladogram / Phylogenetic Tree** — Branching tree where each fork
  represents a common ancestor and branch length may encode evolutionary
  time/distance; leaves are extant taxa. Use for evolutionary-relationship
  content — branch order at each fork is the actual claim being made, get
  it right.
- **Metabolic / Signaling Pathway Diagram** — Nodes are molecules/genes/
  proteins, arrows show reactions/interactions/activation-inhibition
  (often with distinct arrowhead styles for "activates" vs. "inhibits"),
  frequently overlaid on a stylized cell/organelle background for spatial
  context.
- **Punnett Square** — Simple grid, parental alleles on each axis, offspring
  genotype combinations filled into the cells. Exactly one correct layout;
  don't embellish it.
- **Ideogram (Chromosome Diagram)** — Schematic chromosome shape showing
  centromere position and characteristic banding pattern, used to locate
  genes/mutations at specific band coordinates.
- **Anatomical Diagram** — Labeled illustration of a body/organ/system,
  usually with leader lines from labels to precise points on the
  illustration; cross-sections show internal structure at a cut plane. Keep
  label leader lines from crossing each other — reroute or reposition labels
  until they don't.
- **Food Web / Food Chain Diagram** — Directed graph, arrows point from
  prey to predator (the direction energy flows), organized loosely by
  trophic level (producers at the base, apex predators at top). Distinct
  from a simple food chain (a single linear sequence) — a web shows the full
  branching set of relationships in an ecosystem.
- **Cell Diagram** — Labeled cross-section of a cell showing organelles in
  roughly correct relative size/position; specify plant vs. animal cell,
  since the organelle set differs.
- **Life Cycle Diagram** — Circular sequence of stages (egg → larva → pupa →
  adult, etc.), arranged in a ring since the cycle repeats — same shape
  logic as the Hero's Journey circle, applied biologically.

### Chemistry & Molecular Science

- **Skeletal Formula (Line-Angle Structure)** — Standard organic-chemistry
  shorthand: carbon atoms and most hydrogens implied at line vertices/ends,
  only heteroatoms and functional groups explicitly labeled. The default for
  any organic molecule larger than a few atoms.
- **Lewis Structure** — Atoms with explicit bonding lines (single/double/
  triple) and lone-pair dots, used for small molecules where every electron
  matters (not for large organics — use a skeletal formula instead).
- **Reaction Mechanism Diagram** — Sequence of structures connected by
  curved "electron-pushing" arrows showing exactly which electron pair moves
  where, step by step. Use when *how* a reaction proceeds is the content, not
  just reactants-to-products.
- **Protein Contact Map** — Square grid, both axes are residue sequence
  position, a marked cell means those two residues are spatially close in
  the folded structure — a 2D proxy for 3D fold information.
- **Periodic Table Excerpt / Trend Diagram** — Grid arranged by period/group
  with an overlaid arrow or gradient showing a trend (electronegativity,
  atomic radius); only draw the relevant region, not the full table, unless
  the request needs the whole thing.

### Physics, Astronomy & Mathematics

- **Feynman Diagram** — Time and space axes (convention varies), straight/
  wavy/curly lines for different particle types, vertices where lines meet
  representing an interaction. Extremely convention-bound — get line styles
  and vertex rules right rather than improvising.
- **Penrose Diagram (Carter–Penrose)** — Conformally compactified spacetime
  diagram, light rays always at 45°, used to show causal structure/horizons
  in general relativity (black holes, cosmology). Niche and precise — only
  reach for this when the request is specifically about causal structure at
  infinity/horizons, not general spacetime plotting (use a Minkowski
  diagram for that).
- **Minkowski Diagram** — Space on one axis, time on the other, light cones
  at 45°, world lines for objects; the standard special-relativity diagram
  for simultaneity/causality arguments at ordinary (non-cosmological) scale.
- **Free Body Diagram** — A single object drawn as a dot or simple shape with
  labeled force-vector arrows radiating outward (gravity, normal, friction,
  applied force, etc.), arrow length roughly proportional to magnitude.
  The standard first step in any mechanics problem — draw it before any
  equations.
- **Orbital Diagram (Astronomy)** — Central body with elliptical/circular
  paths for orbiting bodies, often with labeled points (perihelion/aphelion,
  apoapsis/periapsis) and direction-of-motion arrows.
- **Hertzsprung–Russell (H-R) Diagram** — Scatter plot of stars, luminosity
  (or absolute magnitude) on the y-axis, temperature/spectral class on the
  x-axis (x-axis conventionally reversed, hot-to-cool left-to-right); the
  main sequence forms a diagonal band. Use for any stellar-evolution
  content — it's the field's canonical chart, don't substitute a generic
  scatter plot.
- **Light Curve** — Line/scatter plot of brightness vs. time for a variable
  star, exoplanet transit, or transient event; a specialized time-series
  chart, mention the domain so it's not confused with a generic line graph.
- **Hasse Diagram** — Directed graph of a partially ordered set (poset) with
  transitive edges omitted and edges implicitly pointing upward (no
  arrowheads needed by convention). Use for lattice/order-theory content —
  divisibility relations, subset lattices.
- **Karnaugh Map (K-Map)** — Grid of 2^n cells (Gray-code ordered so
  adjacent cells differ by one bit) used to visually simplify a Boolean
  expression by grouping adjacent 1s. Purely a simplification tool — always
  show the groupings, not just the filled grid.
- **Vector Field Diagram** — Grid of small arrows, each showing direction
  and magnitude (via length) of a vector quantity (velocity field, force
  field, gradient) at that point in space.
- **Phase Portrait / Phase Space Diagram** — Trajectories plotted in a
  system's state-variable space (e.g. position vs. velocity) rather than
  against time directly, revealing attractors, cycles, and equilibria that a
  plain time-series would hide.

### Linguistics, Humanities & Social Sciences

- **Parse Tree (Syntax Tree)** — Tree diagram with a sentence's root node
  branching down through phrase categories (NP, VP, etc.) to individual
  words at the leaves, per a specific formal grammar. Precise, rule-governed —
  don't freehand the branching, follow the grammar being taught.
- **Stemma Codicum** — Family tree of manuscript versions/copies in textual
  criticism, showing which surviving copies descend from which lost
  originals; structurally a tree diagram but with the specific convention
  of representing *lost/hypothetical* ancestors as distinctly marked
  (dashed/bracketed) nodes.
- **Isogloss Map** — Geographic map with drawn boundary lines separating
  regions that differ in a specific linguistic feature (a word choice, a
  pronunciation), each line a single isogloss; bundles of isoglosses along
  the same boundary indicate a dialect border.
- **Sociogram** — Network graph specialized for social relationships within
  a defined group, nodes are people, directed edges often show a specific
  relation ("chose as friend," "reports conflict with") gathered via
  survey; arrow direction and reciprocity (mutual vs. one-way) are usually
  the interesting signal.
- **Kinship Diagram** — Standardized genealogical notation: triangles for
  male, circles for female, horizontal line for a marriage/partnership,
  vertical line down to offspring, double lines for divorce. Follow the
  standard symbol set exactly — anthropologists and genealogists both read
  it by convention.
- **Argument Map** — Tree/graph of a claim at the root/top, with supporting
  premises and objections as connected boxes below, often color-coded
  (green for support, red for objection) and sometimes with counter-
  objections nested further down. Use for structured debate/critical-
  thinking content, not general concept mapping.
- **Semantic Differential Chart** — Set of horizontal scales, each anchored
  by a pair of opposite adjectives (e.g. "Weak — Strong"), a mark plotted on
  each scale for a rated concept, marks across scales sometimes connected
  into a profile line. Used in psychology/market research for attitude
  measurement.

### Knowledge Representation & Information Architecture

- **Semantic Network** — Node-link graph where nodes are concepts and edges
  are typed semantic relations (is-a, has-a, part-of), core to
  AI/NLP/cognitive-science representations of meaning.
- **Ontology Graph** — Formal hierarchy of classes/categories with explicit
  properties and typed relationships between them (heavier and more formal
  than a semantic network — think OWL/RDF-style class hierarchies, not just
  loose concept links).
- **Concept Lattice** — Formal Concept Analysis diagram: a Hasse-diagram-
  shaped lattice where each node represents a "concept" (a specific set of
  objects sharing a specific set of attributes), nodes ordered by
  generality. Niche and precise — only use when the request is explicitly
  FCA, not a general hierarchy.
- **Topic Map** — ISO-standard structure of topics, associations between
  topics, and occurrences (references to actual source material), used for
  knowledge-base/information-architecture modeling.
- **Site Map / Information Architecture Diagram** — Tree diagram specialized
  for a website/app's page hierarchy, each node a page/screen, showing
  navigation depth and grouping. The UX counterpart to a software
  architecture diagram — use for planning navigation, not code structure.
- **Taxonomy Diagram (Classification Tree)** — See Structures & Hierarchies
  above; in the information-architecture context this specifically models a
  content or product categorization scheme (e.g. an e-commerce category
  tree) rather than a biological or conceptual one.

### Business, Strategy & Operations

- **Service Blueprint** — Extended customer journey map with additional
  rows beneath the customer-facing layer: frontstage actions (what staff/
  interface the customer sees), backstage actions (invisible internal work),
  and supporting processes/systems, with a "line of visibility" separating
  what the customer sees from what they don't. Use when the operational
  machinery behind an experience is the point, not just the customer's
  perception of it.
- **BPMN Diagram (Business Process Model and Notation)** — Formalized
  flowchart dialect with a specific symbol set: rounded rectangles for
  tasks, diamonds with specific glyphs for gateway types (exclusive,
  parallel, inclusive), circles for start/intermediate/end events (with
  different border styles for each), organized into pools/lanes per
  participant. Reach for BPMN specifically when the audience expects formal
  process-notation compliance (enterprise/consulting contexts); a plain
  flowchart is fine otherwise.
- **PERT Chart (Program Evaluation Review Technique)** — Network diagram of
  tasks as nodes (or arrows, in the older ADM convention), connected in
  dependency order, annotated with optimistic/likely/pessimistic duration
  estimates, used to compute the critical path. Distinct from a Gantt chart:
  PERT emphasizes task dependency *structure*, Gantt emphasizes the
  resulting *calendar schedule*.
- **RACI Matrix** — Grid, tasks/deliverables down the rows, roles/people
  across the columns, each cell marked R (Responsible), A (Accountable), C
  (Consulted), or I (Informed). A specialized table more than a diagram, but
  belongs here because it's the standard tool for resolving "who owns this"
  ambiguity — use it explicitly rather than a prose list of responsibilities.
- **Business Model Canvas** — Fixed 9-block grid (Key Partners, Key
  Activities, Key Resources, Value Propositions, Customer Relationships,
  Channels, Customer Segments, Cost Structure, Revenue Streams) filled with
  short notes per block. A fixed template — don't resize or reorder the
  blocks, the layout itself is the recognizable artifact.
- **Lean Canvas** — Sibling of the Business Model Canvas with startup-
  specific blocks (Problem, Solution, Key Metrics, Unfair Advantage instead
  of Key Partners/Resources). Use Lean Canvas over Business Model Canvas
  specifically for early-stage/problem-validation conversations.
- **Value Chain Diagram (Porter's)** — Horizontal arrow-shaped bar divided
  into primary activities (Inbound Logistics → Operations → Outbound
  Logistics → Marketing & Sales → Service) with support activities stacked
  above (Infrastructure, HR, Technology, Procurement), margin shown as a
  wedge at the end. The standard diagram for competitive-strategy analysis
  of where value is added along a firm's operations.
- **Funnel Diagram (Business context)** — See Flow & Spatial below; in a
  business context this is specifically a sales/marketing/conversion funnel,
  often annotated with drop-off percentages between stages.

### Comparison & Ranking

- **Bar Chart (Horizontal)** — One bar per category, length encodes value,
  categories on the vertical axis. Prefer over a vertical column chart the
  moment category labels are long (product names, survey questions) — you
  avoid rotated/truncated text.
- **Column Chart (Vertical Bar)** — Same as above but bars stand vertically;
  best for a modest number of short-labeled categories, or when comparing
  against time buckets (quarters, years) where horizontal reading order
  matches how people expect time to flow.
- **Grouped (Clustered) Bar Chart** — Bars for multiple sub-series placed
  side-by-side within each category cluster; use to compare 2-4 series
  across the same categories where you need to read exact values, not just
  totals.
- **Stacked Bar Chart** — Sub-series bars stacked on top of each other
  within one bar per category, total height = category total. Good for
  showing both the whole and its composition at once, but exact
  sub-segment comparison across categories gets hard once you have more
  than 2-3 segments or the segments don't share a common baseline.
- **Bullet Graph** — Compact single-row chart: a bar for the actual value
  overlaid on background bands (poor/satisfactory/good) plus a tick mark
  for a target, designed by Stephen Few specifically as a dense
  KPI-dashboard replacement for a gauge/speedometer chart.
- **Radar Chart (Spider/Web Chart)** — Multiple axes radiating from a
  center point (one per variable), values plotted and connected into a
  polygon per series. Good for a small number of variables (5-8) compared
  across a small number of series (1-3) — beyond that, overlapping polygons
  become unreadable; use small multiples of radar charts instead of one
  crowded chart.
- **Dumbbell / Connected Dot Plot** — Two dots per category (e.g. "before"
  and "after," or two groups being compared) connected by a line segment,
  making the *gap* between them the visually dominant element. Use
  specifically when the change/difference between two values per category
  is the point, more so than either absolute value.
- **Slope Chart** — Two vertical axes (left = time A, right = time B), one
  line per entity connecting its value at A to its value at B, slope
  direction/steepness showing change. A ranking-change-over-two-points
  specialization — reach for it over a line graph when you specifically
  have only two time points and many entities to compare.

### Trends Over Time

- **Line Graph** — Continuous line connecting data points over an ordered
  (usually time) axis; the default for showing trend/trajectory/velocity of
  a single continuous quantity.
- **Multi-Line Graph** — Several line series on shared axes for direct trend
  comparison; keep it to ~5 lines max before switching to small multiples
  (separate mini-charts per series) — beyond that, color alone can't keep
  lines distinguishable.
- **Area Chart** — Line graph with the region below the line filled in,
  emphasizing cumulative volume/magnitude rather than just the trend shape.
- **Stacked Area Chart** — Multiple series' areas stacked on top of each
  other, showing both a total (top boundary) and composition (band
  thickness) over time. Same caution as stacked bars: sub-series near the
  top of the stack are much harder to compare across time than the bottom
  one.
- **Streamgraph** — Stacked area chart variant with the baseline centered/
  flowing rather than fixed at zero, giving an organic river-like shape.
  Visually striking for showing many overlapping categories' relative
  volume shifting over time, but sacrifices precise value-reading — use for
  a "shape of change" impression, not a chart someone needs to extract
  exact numbers from.
- **Candlestick Chart** — One glyph per time period showing open, high,
  low, close (OHLC) as a body + wicks, body colored by whether close was
  above or below open. The financial-markets standard — don't substitute a
  line graph when open/high/low/close all matter, not just the closing
  value.
- **Sparkline** — Tiny, axis-less inline line chart meant to sit inside a
  sentence, table cell, or dashboard tile — communicates trend shape at a
  glance with zero chrome. Never add axis labels or a legend to a
  sparkline; that defeats its purpose.
- **Cumulative Flow Diagram** — Stacked area chart specialized for
  workflow-stage counts over time (e.g. To Do / In Progress / Done item
  counts), used in Kanban/Agile process analysis; band width at a given
  date = WIP in that stage, band widening over time is a bottleneck signal.
- **Control Chart (Statistical Process Control)** — Line graph of a
  process metric over time with a centerline (mean) and upper/lower control
  limit bands; points outside the bands or non-random patterns within them
  flag process instability. Use for quality/manufacturing-process
  monitoring, not general trend display.

### Part-to-Whole & Composition

- **Pie Chart** — Circle divided into wedges by proportion; keep to 2-6
  categories — more than that and adjacent wedge angles become
  indistinguishable. Order wedges by size (largest starting at 12 o'clock,
  clockwise) unless there's a meaningful non-size ordering (e.g. a Likert
  scale) to preserve instead.
- **Donut Chart** — Pie chart with a hollow center, freeing that center for
  a headline number/KPI; otherwise identical tradeoffs to a pie chart.
- **100% Stacked Bar/Area Chart** — Every bar/area scaled to the same total
  height, showing relative composition (percentages) rather than absolute
  totals — use when the mix matters and the absolute scale doesn't (or is
  shown separately).
- **Treemap** — Nested rectangles, area proportional to value, sized and
  positioned by a tiling algorithm, often colored by a secondary dimension
  and nested for hierarchy (a rectangle inside a rectangle for sub-
  categories). The best part-to-whole chart when you have many categories
  (dozens+) and a real hierarchy — a pie chart falls apart well before a
  treemap does.
- **Waterfall Chart** — Floating bars showing a sequence of positive/
  negative changes bridging a starting total to an ending total (each bar
  starts where the previous one ended); the standard chart for "how did we
  get from X to Y" (a budget bridge, a P&L walk).
- **Icon Array (Pictogram / Isotype Chart)** — Grid of repeated icons, a
  subset shaded/colored to represent a proportion (e.g. "17 of 100 icons
  colored" for 17%). Especially effective for communicating risk/
  probability to a general audience — concrete countable icons read more
  intuitively than an abstract percentage.
- **Marimekko Chart (Mekko Chart)** — Stacked bar chart where *both* bar
  width and segment height vary by value, so area encodes a joint
  distribution of two categorical variables at once (e.g. market share by
  segment, where segment size also varies). Use when both "how big is this
  category" and "how is it split internally" need to be true to scale
  simultaneously — a plain stacked bar only gets the second right.

### Distribution & Range

- **Histogram** — Bars over binned intervals of a continuous variable,
  bar height = frequency/count in that bin; bin width choice materially
  changes the shape shown, so pick a bin width that reveals structure
  without being so fine it just shows noise.
- **Box Plot (Box-and-Whisker Plot)** — Box spans the interquartile range
  (25th-75th percentile) with a line at the median, whiskers extend to a
  defined range (often 1.5×IQR), outliers plotted as individual points
  beyond that. The compact five-number-summary chart for comparing
  distributions across several groups side-by-side.
- **Violin Plot** — Box plot's inner statistics combined with a mirrored
  kernel-density-estimate silhouette showing the full distribution shape
  (bimodal, skewed, etc.) that a box plot's five numbers alone would hide.
- **Density Plot (KDE Plot)** — Smoothed continuous curve estimating a
  variable's probability distribution; use over a histogram when comparing
  multiple overlapping distributions, since overlapping smooth curves stay
  legible where overlapping bars don't.
- **Ridgeline Plot (Joyplot)** — Stack of partially overlapping density
  plots, one per category/time-slice, offset vertically so each peeks out
  above the one behind it. Purpose-built for showing how a distribution's
  shape shifts across many categories/time steps at once (e.g. temperature
  distribution by month).
- **Dot Plot (Strip Plot)** — Individual raw data points plotted along a
  single axis (jittered slightly if needed to avoid full overlap), showing
  every actual value rather than a statistical summary — use for smaller
  datasets where the individual data points themselves are meaningful, not
  just their aggregate shape.
- **Cumulative Distribution Function (CDF) Plot** — Line showing the
  proportion of data at or below each x-value, monotonically rising from 0
  to 1 (or 0-100%). Use when the actual question is "what fraction of
  values are below threshold T," which a CDF answers directly and a
  histogram only answers by eyeballing.

### Relationships & Correlation

- **Scatter Plot** — Points plotted by two numeric variables (x, y), the
  default chart for revealing correlation, clusters, or outliers between
  two continuous quantities.
- **Bubble Chart** — Scatter plot with a third quantitative variable encoded
  as point size (and often a fourth as color). Keep the size-encoding
  linear-in-area, not linear-in-radius — radius-scaling visually
  exaggerates differences.
- **Heatmap (Matrix Heatmap)** — Grid where color intensity encodes a
  value at each row/column intersection; use for correlation matrices,
  time-of-day × day-of-week activity patterns, or any two-categorical-axis
  × one-numeric-value dataset.
- **Parallel Coordinates Plot** — One vertical axis per variable, each data
  row drawn as a connected line crossing all axes at its value on each.
  Good for spotting clusters/outliers across many (5+) dimensions at once,
  at the cost of getting visually dense fast — consider highlighting/
  dimming rather than showing every line at full opacity when there are
  many rows.
- **Connected Scatter Plot** — Scatter plot with points connected in
  temporal order by a line, showing how the relationship between two
  variables evolves over time (as opposed to a plain scatter's single
  static snapshot).
- **Scatter Plot Matrix (SPLOM)** — Grid of small scatter plots, every
  pairwise combination of N variables, diagonal often replaced with a
  histogram/density plot of that single variable. Use for a first-pass
  exploratory look across many variables before deciding which specific
  pairs deserve a full-size scatter plot.
- **Contour Plot** — Lines (or filled bands) connecting points of equal
  value across two continuous input dimensions, like elevation lines on a
  topographic map applied to any bivariate function/density.

### Flow, Process & Spatial Quantity

- **Sankey Diagram** — Flows between nodes drawn as bands whose *width*
  is proportional to quantity, nodes usually arranged left-to-right in
  stages. The standard for showing how a quantity splits, merges, and
  redistributes across stages (energy flow, budget allocation, user-flow
  volume) — width is the entire encoding, keep it accurate and don't let
  bands cross more than necessary.
- **Funnel Chart** — Stacked, narrowing horizontal (or vertical) bands, each
  band a stage in a sequential process, width shrinking as volume drops off
  stage to stage. Use for conversion/attrition processes where the *loss*
  between fixed sequential stages is the point (distinct from a Sankey,
  which can show flows splitting into multiple branches, not just narrowing
  in a single line).
- **Choropleth Map** — Geographic regions (countries, states, counties)
  filled with color/shading proportional to a value. The default for
  region-level statistical geographic data — watch for the classic
  distortion where large-area/low-population regions visually dominate a
  map even when their value is unremarkable; consider a cartogram if that's
  a real concern.
- **Cartogram** — Map where region *area* is distorted to be proportional
  to a data value (population, GDP) rather than true geographic area,
  trading geographic accuracy for value-accuracy. Use specifically to
  correct the large-area-dominates problem a choropleth has.
- **Bubble Map** — Geographic base map with circles sized by value placed
  at specific point locations (cities, facilities); unlike a choropleth,
  this works for point data rather than region-aggregated data.
- **Flow Map** — Geographic map with arrows/lines between locations, line
  width proportional to flow volume (migration, trade, shipping routes) —
  essentially a Sankey diagram laid over real geography instead of an
  abstract left-to-right stage sequence.
- **Dot Density Map** — Each dot represents a fixed unit of a quantity
  (e.g. "1 dot = 100 people"), scattered within the relevant region,
  letting density patterns emerge visually from dot clustering rather than
  from a single aggregate color per region.
- **Isopleth / Contour Map** — Lines connecting points of equal value across
  geography (temperature, elevation, rainfall) — geography's version of a
  contour plot.
- **Transit / Schematic Map** — Deliberately non-geographic, topology-
  preserving map (subway-map style): straight lines at fixed angles
  (usually 45°/90°), stations evenly spaced regardless of true distance,
  optimized purely for route legibility over geographic accuracy.

### Product, UX & Interaction Design

- **User Flow Diagram** — Flowchart specialized for a user's path through a
  product: screens/states as boxes (often with a small wireframe thumbnail),
  user actions as labeled arrows between them, decision points where the
  path branches. Distinct from a sitemap: a user flow follows one goal-
  directed path through the product, a sitemap shows the product's full
  navigable structure.
- **Site Map** — See Information Architecture above.
- **Wireframe** — Low-fidelity screen layout showing structure/placement of
  UI elements (nav, content blocks, CTAs) without visual styling — boxes
  and placeholder text/gray blocks, deliberately unstyled so the
  conversation stays on layout and hierarchy, not color/typography.
- **Empathy Map** — Fixed 4-quadrant template (Says / Thinks / Does /
  Feels) around a central user persona, filled from research notes; a
  specialized 2×2-adjacent grid for synthesizing qualitative user research.
- **Persona Card** — Structured single-panel profile of a fictional/
  composite user: photo/avatar, name, key demographics, goals,
  frustrations, a representative quote. Not really a "diagram" in the
  relational sense, but belongs in a UX deliverable set alongside journey
  maps and empathy maps.
- **Task Flow / Screen Flow Diagram** — Narrower cousin of the user flow,
  scoped to a single task (e.g. "complete checkout") rather than the whole
  product, usually with fewer branches and a tighter linear backbone.

### Manufacturing, Engineering & Supply Chain

- **Process Flow Diagram (PFD)** — Industrial-engineering flowchart showing
  major process units/equipment and the material streams between them,
  often annotated with flow rates/conditions; the high-level cousin of a
  P&ID.
- **Piping and Instrumentation Diagram (P&ID)** — Highly detailed schematic
  of piping, equipment, valves, and instrumentation using a standardized
  symbol library. Extremely convention-bound engineering documentation —
  don't improvise symbols, use the standard ISA/ISO set the domain expects.
- **Exploded-View Diagram** — 3D (or pseudo-3D isometric) view of an
  assembly with each component pulled apart along a shared axis, dashed
  lines showing how they reassemble; the standard for assembly
  instructions and parts catalogs.
- **Kinematic Diagram** — Simplified schematic of a mechanism's moving
  parts (links, joints, cams) using standard symbols for each joint type
  (revolute, prismatic, etc.), showing degrees of freedom rather than
  literal appearance.
- **Bill of Materials (BOM) Tree** — Hierarchical decomposition of a
  product into its constituent parts and sub-assemblies down to individual
  components, each node annotated with part number/quantity. Structurally a
  tree diagram/work-breakdown-structure applied to physical parts.
- **Supply Chain Map** — Node-link or geographic-flow diagram of suppliers →
  manufacturing → distribution → retail, often overlaid on a geographic
  base map when physical location/logistics matter, or abstracted to a
  flowchart when only sequence/relationship matters.
- **Factory / Facility Layout Diagram** — Scaled floor-plan-style diagram
  showing physical placement of equipment/stations and the material flow
  path between them; used for layout optimization, distinguish from a
  process flow diagram which is topological, not spatially accurate.

### Finance & Economics

- **Supply and Demand Curve** — Two intersecting curves (supply upward-
  sloping, demand downward-sloping) on price (y-axis) vs. quantity (x-axis)
  axes, intersection marks equilibrium price/quantity; shift arrows show
  the effect of external changes. The canonical introductory-economics
  diagram — get curve direction and the equilibrium point exactly right.
- **Production Possibility Frontier (PPF)** — Concave curve on axes of two
  goods' output quantities, showing the maximum achievable combination
  given fixed resources; points inside are feasible-but-inefficient, points
  outside are currently unattainable.
- **Indifference Curve Diagram** — Set of curves (each representing equal
  utility) on two-goods axes, plus a budget-constraint line, tangency point
  marking the optimal consumption bundle.
- **Lorenz Curve** — Cumulative share of income/wealth (y) vs. cumulative
  share of population (x), compared against a 45° line of perfect
  equality; the gap between the curve and that line is the visual basis for
  the Gini coefficient. Use specifically for inequality-measurement
  content.
- **Yield Curve** — Line plot of interest rate (y) vs. time-to-maturity (x)
  for bonds of the same credit quality; a flat or inverted shape (short-
  term rates above long-term) is itself the notable signal readers look for
  — label the curve's shape explicitly if it's inverted.
- **Cap Table Waterfall** — Waterfall chart variant showing how exit
  proceeds distribute across share classes in liquidation-preference order,
  each bar/segment a class's payout, running total bridging to the final
  distribution.
- **Candlestick Chart** — See Trends Over Time above; the finance-specific
  application of OHLC data.
- **Correlation Matrix (Asset/Portfolio context)** — See Relationships &
  Correlation above; in a finance context this specifically screens
  pairwise asset-return correlation for diversification analysis.

### Environmental & Earth Science

- **Carbon Cycle Diagram** — Node-link diagram of carbon reservoirs
  (atmosphere, ocean, biomass, soil, fossil fuels) connected by labeled
  flow arrows (photosynthesis, respiration, combustion), arrow width
  optionally scaled to flux magnitude — structurally a Sankey-adjacent flow
  diagram specialized for biogeochemical cycles.
- **Watershed / Drainage Basin Map** — Geographic map with the basin
  boundary outlined and the stream/river network drawn as branching lines
  converging toward the outlet, often shaded by elevation/contour.
- **Climate Stripes** — Single row (or grid of rows) of solid color bars,
  one per year, colored on a single blue-to-red scale by temperature
  anomaly, deliberately stripped of axes/gridlines/labels beyond a year
  range — a minimalist climate-trend communication format, not a
  data-analysis chart; don't add extra chart chrome to it.
- **Geologic Cross-Section** — Side-view slice through the earth showing
  rock-layer (stratum) boundaries, folding/faulting, using standardized
  rock-type pattern fills; vertical exaggeration (if used) should be
  explicitly labeled since it distorts true dip angles.
- **Trophic Pyramid (Ecological Pyramid)** — Pyramid diagram (see Structures
  & Hierarchies) specialized for energy/biomass/numbers at each trophic
  level, narrowing upward — the ecology-specific instance of the general
  pyramid pattern.

### Music & Audio

- **Musical Staff / Score Notation** — Standard five-line staff with notes,
  rests, clefs, key/time signatures — use actual notation conventions, not
  an approximation, whenever pitch and rhythm both need to be precisely
  represented.
- **Chord Progression Diagram** — Sequence of chord symbols over a
  timeline/measure grid, sometimes paired with a circle-of-fifths
  reference to show harmonic relationships between the chords used.
- **Circle of Fifths** — Fixed circular arrangement of the 12 pitch
  classes/keys in fifths order, major keys on the outer ring and relative
  minors on an inner ring; a completely standardized reference diagram —
  don't reorder the keys.
- **Waveform Diagram** — Amplitude vs. time plot of an audio signal; use for
  content about loudness/dynamics/timing at the signal level.
- **Spectrogram** — Frequency (y) vs. time (x) with color/intensity encoding
  amplitude at each frequency-time cell; use when the frequency content
  over time (not just overall loudness) is the point — a waveform can't
  show that.
- **Song Structure Diagram** — Horizontal timeline divided into labeled
  sections (Intro / Verse / Chorus / Bridge / Outro), block width
  proportional to section duration; effectively a specialized timeline for
  arrangement/form analysis.

### Sports, Games & Competition

- **Play Diagram ("Xs and Os")** — Schematic field/court with player
  positions marked (X/O per team) and route/movement arrows showing a
  specific planned play; standardized per sport (football routes, basketball
  screens/cuts) — follow the sport's own symbol conventions.
- **Bracket / Tournament Diagram** — See Structures & Hierarchies above;
  reiterated here because it's the default sports/competition application.
- **Heat Map (Positional/Spatial, Sports context)** — Court/field/pitch
  base image overlaid with a color-intensity heatmap of where a player or
  event (shots, touches) concentrated — the sports-analytics application of
  the general heatmap.
- **Payoff Matrix (Game Theory)** — Grid, one player's strategies as rows,
  the other's as columns, each cell holding both players' payoffs for that
  strategy combination. The standard 2-player game-theory diagram — always
  label whose payoff is listed first/second in each cell.
- **Game Tree (Extensive Form)** — Tree diagram where each node is a
  decision point for a specific player, branches are their available
  moves, and leaves are terminal outcomes/payoffs; use over a payoff
  matrix specifically when moves happen sequentially rather than
  simultaneously.
- **Standings / League Table** — Ranked table more than a diagram, but the
  domain-standard artifact for competition state (position, wins/losses,
  points) — mention it as the right choice when a request for a "sports
  chart" is actually asking for ranked results, not a spatial diagram.

### Legal, Compliance & Governance

- **Decision/Compliance Flowchart** — Standard flowchart (see Processes &
  Workflows) applied to a regulatory/compliance decision path (e.g. "is
  this transaction reportable") — precision in the decision diamonds
  matters more here than almost anywhere else, since real compliance
  outcomes hang on the exact branching logic.
- **Org Chart of Authority / Delegation** — Org chart variant explicitly
  annotated with signing/approval authority levels or delegated powers at
  each box, rather than just reporting lines.
- **Case/Precedent Timeline** — Timeline (see Time, Journey & Narrative)
  specialized for legal case history, milestones being filings/rulings/
  appeals rather than generic events.
- **RACI Matrix (Governance context)** — See Business, Strategy & Operations
  above; frequently reused in compliance/governance docs for regulatory
  accountability mapping.

### Data Science & Machine Learning

- **Neural Network Architecture Diagram** — Layered node-link diagram:
  columns of nodes (neurons/layers) fully or sparsely connected to the next
  column, input layer on one side, output on the other; for modern deep
  architectures, prefer a block diagram (labeled boxes for Conv/Attention/
  Pooling/etc. layers with shape annotations) over drawing individual
  neurons once the network is deep — individual-neuron diagrams stop being
  legible past a couple of layers.
- **Confusion Matrix** — Square grid, actual class down the rows, predicted
  class across columns, cell value = count (or normalized rate); the
  standard classifier-evaluation diagram — always specify which axis is
  actual vs. predicted, conventions vary.
- **ROC Curve** — True positive rate (y) vs. false positive rate (x) as
  the classification threshold sweeps, compared against a diagonal
  no-skill baseline; area under the curve (AUC) is the usual accompanying
  summary statistic.
- **Decision Boundary Plot** — 2D scatter plot of data points colored by
  class, overlaid with a shaded/contoured region showing where a trained
  classifier draws its decision boundary — use specifically to explain
  *how* a model separates classes, not just how accurately.
- **Pipeline/DAG Diagram (ML context)** — See Computing above; in an ML
  context nodes are typically preprocessing/training/evaluation stages,
  often drawn left-to-right as a linear-with-branches pipeline.
- **Data Lineage Diagram** — Directed graph tracing a dataset/field from
  its origin through every transformation to its final consuming
  system/report — a DAG specialized for provenance/audit rather than
  execution order.

### Genealogy & Family

- **Family Tree (Pedigree Chart)** — Standard genealogical tree, either
  ancestor-focused (fan or pedigree chart, one person's ancestors branching
  backward) or descendant-focused (branching forward from a founding
  couple); use the kinship-diagram symbol conventions (see Linguistics &
  Social Sciences above) when marriages/multiple unions need explicit
  representation, plain tree branching when only lineage matters.

---

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
static image." Otherwise, default to using whatever live/interactive/animated
capability the chosen tool exposes.

## Step 4 — Execute

1. Pick type (Step 1) + tool (Step 2).
2. Build the diagram using the tool's own skill for exact syntax:
   `skill(action=read, name=excalidraw)` / `name=tldraw-offline` / `name=penecho` / `name=diagram`.
3. Apply the interactivity/animation mandate (Step 3) — do not skip this check.
4. Actually create + open it. Never stop at "here's what I'd draw" — the tools
   open the result for the user automatically; use them.
