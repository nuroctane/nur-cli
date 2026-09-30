# Table Partitioning & Clustering Strategy

Architectural guidelines, decision matrices, and DDL patterns for table design,
query pruning optimization, and storage efficiency.

## Table of Contents

-   [1. Decision Matrix: Partitioning vs. Clustering vs. Hybrid](#1-decision-matrix-partitioning-vs-clustering-vs-hybrid) (Lines 21-31)
-   [2. Partitioning Design Principles](#2-partitioning-design-principles) (Lines 33-65)
    -   [Partitioning Types by Column Type](#partitioning-types-by-column-type) (Lines 35-45)
    -   [Core Partitioning Invariants & Rules](#core-partitioning-invariants-rules) (Lines 47-65)
-   [3. Clustering Design Principles](#3-clustering-design-principles) (Lines 67-85)
    -   [Clustering Rules & Mechanics](#clustering-rules-mechanics) (Lines 69-85)
-   [4. Production DDL Templates](#4-production-ddl-templates) (Lines 87-161)
    -   [Recommended Hybrid Pattern (Partition + Cluster)](#recommended-hybrid-pattern-partition-cluster) (Lines 89-107)
    -   [Migrating an Existing Unpartitioned Table to Partitioned](#migrating-an-existing-unpartitioned-table-to-partitioned) (Lines 109-118)
    -   [Modifying the Clustering Specification](#modifying-the-clustering-specification) (Lines 120-161)
-   [5. Table Design & Recommendation Workflow (Given a Table or Schema)](#5-table-design-recommendation-workflow-given-a-table-or-schema) (Lines 163-206)
-   [6. Table Design Anti-Patterns & Architecture Constraints](#6-table-design-anti-patterns-architecture-constraints) (Lines 208-234)

## 1. Decision Matrix: Partitioning vs. Clustering vs. Hybrid

<!-- mdformat off -->
| Dimension | Partitioning (`PARTITION BY`) | Clustering (`CLUSTER BY`) | Hybrid (`PARTITION BY` + `CLUSTER BY`) |
| :--- | :--- | :--- | :--- |
| **Best For** | Low-cardinality values, temporal/date boundaries, date-based TTL retention. | High-cardinality values, multiple filter/group-by columns, frequent DML modifications. | Large temporal tables with multi-dimensional filtering (e.g., date + tenant/status). |
| **Pruning Mechanism** | **Partition Pruning:** Whole partitions are eliminated from scanning before execution. | **Block Pruning:** Capacitor data blocks within files are skipped dynamically during execution. | Two-tier pruning: Partition pruning narrows the time window, then block pruning skips blocks within partitions. |
| **Cost Estimation** | Known deterministically before query execution via dry-run (`totalBytesProcessed`). | Estimated dynamically during execution as blocks are evaluated. | Upfront partition scan bounds with execution-time block savings. |
| **Storage Lifecycle** | Unlocks **partition-level TTL** and 90-day long-term 50% storage price discount. | No partition-level expiration (data is co-located across storage blocks). | Enables partition TTL retention while optimizing intra-partition query performance. |
| **Limits** | Up to **10,000 partitions per table**. | Up to **4 clustering columns** per table. | 10,000 partition limit applies; clustering operates within each partition. |
<!-- mdformat on -->

## 2. Partitioning Design Principles

### Partitioning Types by Column Type

*   **`TIMESTAMP` Column:** `PARTITION BY TIMESTAMP_TRUNC(column_name, DAY)`
*   **`DATE` Column:** `PARTITION BY date_column` (or for monthly granularity:
    `PARTITION BY DATE_TRUNC(date_column, MONTH)`)
*   **`DATETIME` Column:** `PARTITION BY DATETIME_TRUNC(datetime_column, DAY)`
*   **Ingestion-Time Partitioning:** BigQuery assigns rows to partitions based
    on the ingestion time using the pseudo-column `_PARTITIONDATE` /
    `_PARTITIONTIME`.
*   **Integer-Range Partitioning:** Partition by an `INT64` column using
    `RANGE_BUCKET(column, GENERATE_ARRAY(start, end, interval))`.

### Core Partitioning Invariants & Rules

*   **10,000 Partition Quota Limit:** Each partitioned table can have up to
    10,000 partitions. If a table reaches or approaches this limit:
    *   *Solution:* Widen the partition time unit (e.g., switch from `HOUR` to
        `DAY`, or `DAY` to `MONTH`) and **cluster on the granular time column**
        (e.g. partition by month, cluster by timestamp). BigQuery automatically
        clusters data blocks within each partition.
*   **Enforce Partition Pruning (`require_partition_filter = true`):** For large
    tables, always set `require_partition_filter = true` to force queries to
    supply a partition predicate in the `WHERE` clause, preventing accidental
    multi-terabyte unpruned full-table scans.
*   **Avoid Over-Partitioning:** Do not create hourly partitions for low-volume
    tables. Partitioning that results in a small amount of data per partition
    (approximately less than 10 GB) increases the table's metadata, and can
    affect metadata access times when querying the table.
*   **Long-Term Storage Discount:** Partitions that are not modified for 90
    consecutive days automatically qualify for **50% cheaper long-term storage**
    pricing.

## 3. Clustering Design Principles

### Clustering Rules & Mechanics

*   **Up to 4 Columns:** Specify up to 4 columns in the `CLUSTER BY` clause.
*   **Column Ordering is Critical:** BigQuery clusters data hierarchically based
    on the order of columns specified:
    *   Place the **most frequently filtered column** or the column with
        **equality filters** first (`col1`).
    *   Place secondary filter columns, range filter columns, or `GROUP
        BY`/`JOIN` keys next (`col2`, `col3`, `col4`).
    *   *Trap:* Filtering on `col2` without a predicate on `col1` significantly
        diminishes block pruning effectiveness.
*   **Supported Data Types:** `STRING` (first 1,024 characters), `INT64`,
    `NUMERIC`, `BIGNUMERIC`, `BOOL`, `TIMESTAMP`, `DATE`, `DATETIME`,
    `GEOGRAPHY`, `RANGE`.
*   **Automatic Re-Clustering:** BigQuery performs continuous, fully-managed
    background reclustering to maintain block sortedness as new data is appended
    or modified, at **zero slot cost**.

## 4. Production DDL Templates

### Recommended Hybrid Pattern (Partition + Cluster)

```sql
CREATE OR REPLACE TABLE `{project_id}.{dataset_id}.events`
(
  event_id STRING,
  customer_id STRING,
  event_type STRING,
  event_timestamp TIMESTAMP,
  payload JSON
)
PARTITION BY TIMESTAMP_TRUNC(event_timestamp, DAY)
CLUSTER BY customer_id, event_type
OPTIONS (
  require_partition_filter = true,
  partition_expiration_days = 365,
  description = 'High-volume events partitioned by day and clustered by customer'
);
```

### Migrating an Existing Unpartitioned Table to Partitioned

```sql
-- Unpartitioned tables cannot be converted via ALTER TABLE; use CTAS to create the partitioned version:
CREATE OR REPLACE TABLE `{project_id}.{dataset_id}.events_partitioned`
PARTITION BY TIMESTAMP_TRUNC(event_timestamp, DAY)
CLUSTER BY customer_id, event_type
OPTIONS (require_partition_filter = true)
AS SELECT * FROM `{project_id}.{dataset_id}.events_unpartitioned`;
```

### Modifying the Clustering Specification

Apply a new clustering specification to unpartitioned or partitioned tables in
two steps. First, update the clustering specification of the table to match the
new clustering:

```bash
bq update --clustering_fields=customer_id,status \
  {project_id}:{dataset_id}.{table_id}
```

The `--clustering_fields` value is a comma-separated list of column names to use
for clustering.

Then, to cluster all rows according to the new clustering specification, run the
following `UPDATE` statement. Propose this statement to the user rather than
running it directly:

```sql
UPDATE `{project_id}.{dataset_id}.{table_id}`
SET customer_id = customer_id
WHERE true;
```

The `UPDATE` must supply a partition filter if the table has
`require_partition_filter = true`, and a single job cannot modify more than
4,000 partitions (the per-job modification limit). For partitioned tables,
re-cluster in bounded batches:

```sql
UPDATE `{project_id}.{dataset_id}.{table_id}`
SET customer_id = customer_id
WHERE {partition_column} >= '{range_start}'
  AND {partition_column} < '{range_end}';
```

Repeat over consecutive ranges of fewer than 4,000 partitions until the whole
table is covered.

Tell the user that applying a new clustering specification to a table in
long-term storage reverts that table to
[active storage pricing](https://cloud.google.com/bigquery/pricing#storage-pricing).

## 5. Table Design & Recommendation Workflow (Given a Table or Schema)

When asked to recommend a partitioning and clustering strategy for a specific
table or schema:

1.  **Step 1: Identify Partitioning Key & Time Granularity**

    *   *Scan Column Types:* Look for `TIMESTAMP`, `DATE`, or `DATETIME` columns
        representing primary event generation time.
    *   *Check the 10,000 Partition Quota Limit:*
        *   **`DAY` Partitioning (Default):** Optimal for 1–7 year lifespans
            (~365 to ~2,555 partitions).
        *   **`HOUR` Partitioning:** Choose it only for tables with a high
            volume of data spanning a short date range — typically less than six
            months of timestamp values. Make sure the resulting partition count
            stays within the 10,000 partition limit.
        *   **`MONTH` or `YEAR` Partitioning:** For historical archives (>10
            years), partition by `MONTH` and cluster by the timestamp.
    *   *No Temporal Column:* Evaluate integer range partitioning
        (`RANGE_BUCKET`) for incremental IDs, or ingestion-time partitioning
        (`_PARTITIONTIME`) for append-only log streams.

2.  **Step 2: Select & Order Clustering Columns (Up to 4 Columns)**

    *   *Candidate Columns:* High-cardinality dimension columns frequently
        present in `WHERE` equality filters, `JOIN ON` clauses, or `GROUP BY`
        aggregations (e.g., `customer_id`, `organization_id`, `status`).
    *   *Column Ordering Rules:*
        *   `col1`: The most frequently filtered column or equality/tenant
            identifier (`customer_id`).
        *   `col2`, `col3`: Secondary dimensions or range filter attributes
            (`event_type`, `created_at`).
        *   *Order Trap:* Queries filtering on `col2` without a predicate on
            `col1` cannot perform efficient block pruning.

3.  **Step 3: Output Production DDL & Guardrails**

    *   Generate complete `CREATE TABLE` DDL, including CTAS migrations. To
        modify a table's clustering specification, use the two-step sequence in
        Section 4.
    *   Add `require_partition_filter = true` for high-volume production tables
        to prevent accidental unpruned full-table scans.
    *   Add `partition_expiration_days = N` for staging, ETL, or rolling log
        tables to automate data cleanup.

## 6. Table Design Anti-Patterns & Architecture Constraints

-   **`ALTER TABLE` Partitioning Invariant:** BigQuery **does not support
    altering an existing unpartitioned table to become partitioned**. You must
    create a new partitioned table using CTAS or a load job. `ALTER TABLE` does
    not support setting or changing a table's clustering; use `bq update
    --clustering_fields` (Section 4).
-   **Small-Partition Anti-Pattern (<10 GB per Partition):** Prefer clustering
    over partitioning when partitioning would yield **less than ~10 GB per
    partition**. Creating many small partitions inflates table metadata and
    slows metadata access at query time without yielding scan savings.
    *(Separately, on-demand query compute charges enforce a 10 MiB data
    processed minimum per referenced table and per query, regardless of table or
    partition size).*
-   **1,024-Character String Limit:** For `STRING` clustering columns, BigQuery
    evaluates only the **first 1,024 characters** for block pruning. Do not
    cluster on long free-text descriptions or raw JSON payloads; extract concise
    identifiers.
-   **Streaming Buffer Compaction (`__UNPARTITIONED__`):** Rows recently
    streamed via the BigQuery Storage Write API initially reside in the
    `__UNPARTITIONED__` partition with a `NULL` `_PARTITIONTIME`. These rows
    cannot be pruned by partition filters until extracted into permanent
    partitions by BigQuery's background compaction.
-   **Query Filter Optimization:** To leverage partition and clustering pruning
    effectively in query execution, predicates must use compile-time constants.
    For query-level `WHERE`-clause rewrites and function anti-patterns, refer to
    the `bigquery-optimization` skill.
