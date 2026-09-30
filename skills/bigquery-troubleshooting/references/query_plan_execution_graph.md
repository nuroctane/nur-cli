# BigQuery Troubleshooting: Execution Graph & Query Plan

This reference guide provides diagnostic workflows and grounding concepts for
investigating single-job performance bottlenecks, execution graphs, stage-level
operations, and query plan metrics in BigQuery.

## Table of Contents

-   [Diagnostic Workflows](#diagnostic-workflows) (Lines 15-48)
    -   [1. Single-Job Stage Bottleneck Triage & REST API Lookup](#1-single-job-stage-bottleneck-triage-rest-api-lookup) (Lines 17-37)
    -   [2. General Telemetry & Stage Metrics Cross-Reference](#2-general-telemetry-stage-metrics-cross-reference) (Lines 39-48)
-   [Execution Graph & UI Grounding Concepts](#execution-graph-ui-grounding-concepts) (Lines 50-96)
-   [Strict Disambiguation Invariants](#strict-disambiguation-invariants) (Lines 98-113)

## Diagnostic Workflows

### 1. Single-Job Stage Bottleneck Triage & REST API Lookup

-   **When to Use / Symptoms**: A query is slow or stalled, and you need to
    isolate whether delay occurred during pending/queueing or within a specific
    execution stage.
-   **Diagnostic Procedure**:
    -   Use the REST API single-job point-lookup via `bq show
        --location={location} --format=prettyjson -j {project_id}:{job_id}` to
        retrieve detailed per-stage execution metrics, timeline events, and
        performance insights. See the `bigquery-observability` skill
        (`references/job_performance_queries.md`, section "REST API Single-Job
        Point-Lookup & Stage Bottlenecks") for the CLI command and field
        definitions.
    -   Inspect `statistics.query.queryPlan` for stage-level compute, input,
        output, and shuffle skew.
    -   **Bottleneck Determination**: Calculate stage duration as `duration =
        end_ms - start_ms`.
    -   **Optimization Target**: Clarify whether the user is optimizing for
        **Slot Time** (Reservations / Flat-rate / Editions) or **Bytes
        Processed** (On-demand pricing) to prioritize the corresponding
        bottleneck stages.

### 2. General Telemetry & Stage Metrics Cross-Reference

For full SQL schema dictionaries, physical units, and column definitions (such
as `records_read`, `records_written`, `shuffle_output_bytes_spilled`, and
`slot_ms` across `job_stages` and `INFORMATION_SCHEMA.JOBS`), see the
`bigquery-observability` skill:

-   `references/schema_compute.md` for raw telemetry metrics and column types.
-   `references/job_performance_queries.md` for SQL-based stage bottleneck
    analysis and historical baseline comparisons.

## Execution Graph & UI Grounding Concepts

Base all terminology explanations and console visualization guidance on the
official definitions below:

-   **execution graph** — The visual representation of the query plan stages and
    steps in the BigQuery Console.
    [Docs](https://cloud.google.com/bigquery/docs/query-plan-explanation#understand_the_execution_graph)
-   **query plan** — The series of stages and steps BigQuery generates and
    executes to run a query.
    [Docs](https://cloud.google.com/bigquery/docs/query-plan-explanation)
-   **stage** — A single execution unit in a query plan containing multiple
    operations/steps executed by worker slots.
    [Docs](https://cloud.google.com/bigquery/docs/query-plan-explanation#stage-overview)
-   **step** — An individual operation within a stage (such as `READ`, `WRITE`,
    `AGGREGATE`, `JOIN`, or `FILTER`).
    [Docs](https://cloud.google.com/bigquery/docs/query-plan-explanation#per-stage_step_information)
-   **substep / substep variables** — Intermediate sub-operations within a stage
    step passing data via intermediate variables (such as `$1`, `$2`). These
    variables are unique to that stage and pass expressions or intermediate
    aggregations between operations.
    [Docs](https://cloud.google.com/bigquery/docs/query-plan-explanation#per-stage_step_information)
-   **slowest stage / bottleneck** — The stage causing the highest latency or
    consuming the largest share of slot resources. Calculate duration as
    `end_ms - start_ms`.
    [Docs](https://cloud.google.com/bigquery/docs/query-plan-explanation#stage-overview)
-   **summary panel** — Top-level summary panel in BigQuery Console displaying
    overall job duration, bytes processed, and slot usage.
    [Docs](https://cloud.google.com/bigquery/docs/query-plan-explanation)
-   **stage details panel** — Side panel in BigQuery Console displaying granular
    stage metrics, worker compute/wait breakdown, and operations.
    [Docs](https://cloud.google.com/bigquery/docs/query-plan-explanation#navigating_the_execution_graph)
-   **input stage** — The predecessor stage that provides input data to the
    current stage.
    [Docs](https://cloud.google.com/bigquery/docs/query-plan-explanation#understand_the_execution_graph)
-   **output stage** — The successor stage that consumes data produced by the
    current stage.
    [Docs](https://cloud.google.com/bigquery/docs/query-plan-explanation#understand_the_execution_graph)
-   **parallel inputs** — The number of workers concurrently processing input
    partitions within a stage.
    [Docs](https://cloud.google.com/bigquery/docs/query-plan-explanation#stage-overview)
-   **completed parallel inputs** — The number of worker tasks that have
    finished execution within a stage.
    [Docs](https://cloud.google.com/bigquery/docs/query-plan-explanation#stage-overview)
-   **cache hit** — Indicates whether the query result was served from BigQuery
    cached results (0 bytes billed).
    [Docs](https://cloud.google.com/bigquery/docs/query-plan-explanation)

## Strict Disambiguation Invariants

-   **Mandatory Correction (Bytes Scanned in Stage)**: BigQuery **does NOT**
    track *Bytes Scanned* per individual stage. When asked, issue the
    correction:

    > ⚠️ **Correction**: BigQuery does not track *Bytes Scanned* per individual
    > stage. Instead, it tracks **Records Read** (the volume of input rows
    > processed by the stage).

-   **Substep Variables (`$1`, `$2`)**: When asked why steps contain `$` signs,
    clarify that these represent intermediate **substep variables** unique to
    that stage passing data between operations.

-   **Citation Requirement**: Always cite the official documentation link for
    every execution graph concept or metric discussed.
