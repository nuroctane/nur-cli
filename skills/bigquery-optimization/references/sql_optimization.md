# BigQuery SQL Optimization

Guidance for optimizing the user's SQL by applying a list of optimization rules.
Apply any of the rules listed below to the provided SQL. If none of the rules
apply, tell the user that their query is already optimal. Only consult the list
of rules in the subsections in this file.

> [!TIP] Always include a **"Summary of Optimizations"** section listing only
> the optimizations applied.

## Table of Contents

-   [SQL optimization rules for reducing slot-time](#sql-optimization-rules-for-reducing-slot-time) (Lines 30-438)
    -   [Materialize a CTE with temporary tables](#materialize-a-cte-with-temporary-tables) (Lines 32-72)
    -   [Order multiple temporary tables or variables in reference order](#order-multiple-temporary-tables-or-variables-in-reference-order) (Lines 74-139)
    -   [Add DISTINCT to semijoin subqueries](#add-distinct-to-semijoin-subqueries) (Lines 141-187)
    -   [Replace REGEXP_CONTAINS with LIKE](#replace-regexp_contains-with-like) (Lines 189-207)
    -   [In a WHERE clause, use a BETWEEN expression instead of the EXTRACT function](#in-a-where-clause-use-a-between-expression-instead-of-the-extract-function) (Lines 209-242)
    -   [Add a LIMIT clause to an ORDER BY clause](#add-a-limit-clause-to-an-order-by-clause) (Lines 244-274)
    -   [Remove an ORDER BY clause from a CREATE TABLE statement](#remove-an-order-by-clause-from-a-create-table-statement) (Lines 276-306)
    -   [Compare date columns to DATE literals, not string casts](#compare-date-columns-to-date-literals-not-string-casts) (Lines 308-338)
    -   [Add a null filter to a NOT IN subquery](#add-a-null-filter-to-a-not-in-subquery) (Lines 340-369)
    -   [Replace a WHERE IN OR IN clause with LEFT JOINs](#replace-a-where-in-or-in-clause-with-left-joins) (Lines 371-407)
    -   [Replace exact aggregates with approximate aggregates](#replace-exact-aggregates-with-approximate-aggregates) (Lines 409-438)
-   [SQL optimization rules for reducing amount of data](#sql-optimization-rules-for-reducing-amount-of-data) (Lines 440-639)
    -   [Replace `SELECT *` with a specific column list](#replace-select-with-a-specific-column-list) (Lines 442-479)
    -   [Check the query filters on the partitioned column](#check-the-query-filters-on-the-partitioned-column) (Lines 481-639)
        -   [Fetching table schemas from BigQuery](#fetching-table-schemas-from-bigquery) (Lines 611-639)

## SQL optimization rules for reducing slot-time

### Materialize a CTE with temporary tables

If there is a CTE clause that is referenced multiple times, suggest
materializing it into a temporary table by wrapping the CTE clause in a `CREATE
TEMP TABLE AS` statement.

Rationale: CTE clauses are evaluated every time a query references them;
materializing a CTE into a temporary table means that it will only be evaluated
once.

Here is an example of a query where multiple CTEs are referenced multiple times:

```sql
WITH t1 AS (SELECT a0 FROM a),
     t2 AS (SELECT x.a0 FROM t1, t1 x),
     t3 AS (SELECT x.a0 FROM t2, t2 x),
     t4 AS (SELECT x.a0 FROM t3, t3 x)
SELECT
  a0,
  COUNT(*)
FROM
  t4
GROUP BY
  1;
```

Here is how the rewritten query should look:

```sql
CREATE TEMP TABLE t1 AS (SELECT a0 FROM a);
CREATE TEMP TABLE t2 AS (SELECT x.a0 FROM t1, t1 x);
CREATE TEMP TABLE t3 AS (SELECT x.a0 FROM t2, t2 x);
CREATE TEMP TABLE t4 AS (SELECT x.a0 FROM t3, t3 x);
SELECT
  a0,
  COUNT(*)
FROM
  t4
GROUP BY
  1;
```

### Order multiple temporary tables or variables in reference order

When applying the rules for materializing CTEs into temporary tables (see
subsection "Materialize a CTE with temporary tables") or declaring variables
(see rule 3.b. in the subsection "Check the query filters on the partitioned
column"), order them in the order that they are referenced in the query.
Temporary tables and variables must come before other temporary tables or
variables that reference them.

Example of a query with a temporary table and a variable that is out of order:

```sql
DECLARE last_order_date DEFAULT (
  SELECT
    max(date)
  FROM
    quantity_sold_per_day
);

CREATE TEMP TABLE quantity_sold_per_day AS (
  SELECT
    EXTRACT(DATE from timestamp) AS date,
    SUM(quantity) as total_quantity
  FROM
    orders
  GROUP BY 1
);

SELECT
  timestamp,
  order_id,
  quantity
FROM
  orders
WHERE
  timestamp >= last_order_date;
```

Here is the corrected order of the temporary table and variable:

```sql
CREATE TEMP TABLE quantity_sold_per_day AS (
  SELECT
    EXTRACT(DATE from timestamp) AS date,
    SUM(quantity) as total_quantity
  FROM
    orders
  GROUP BY 1
);

DECLARE last_order_date DEFAULT (
  SELECT
    max(date)
  FROM
    quantity_sold_per_day
);

SELECT
  timestamp,
  order_id,
  quantity
FROM
  orders
WHERE
  timestamp >= last_order_date;
```

### Add DISTINCT to semijoin subqueries

If the query contains a semijoin, rewrite the subquery to have a DISTINCT clause
only if the subquery does not have a GROUP BY clause. If the subquery has a
GROUP BY clause, then the subquery should remain as is. If the subquery does not
have a GROUP BY clause, then the subquery should be rewritten to have a DISTINCT
clause.

Rationale: `DISTINCT` in the semijoin subquery reduces the amount of data to
check for in the `IN` clause.

Here is an example of a query with a semijoin whose subquery does not have a
GROUP BY clause:

```sql
SELECT
  t1.a0
FROM
  test.a t1
WHERE
  t1.a2 IN (SELECT t2.a2 FROM test.a t2);
```

Here is the query rewritten with a DISTINCT clause:

```sql
SELECT
  t1.a0
FROM
  test.a AS t1
WHERE
  t1.a2 IN (SELECT DISTINCT t2.a2 FROM test.a t2);
```

Here is an example of a query with a semijoin whose subquery has a GROUP BY
clause and should not be rewritten:

```sql
SELECT
  t1.a0
FROM
  test.a t1
WHERE
  t1.a2 IN (SELECT t2.a2, count(*) FROM test.a t2 group by 1);
```

This query should not be rewritten since the subquery has an aggregation.

### Replace REGEXP_CONTAINS with LIKE

If the query contains the function `REGEXP_CONTAINS` where the regular
expression uses wildcards, rewrite the query to use the `LIKE` operator instead.

Rationale: `LIKE` is more performant than `REGEXP_CONTAINS`.

Examples of valid rewrites:

*   `REGEXP_CONTAINS(a, '.*abc.*')` should be rewritten as `a LIKE '%abc%'`
*   `REGEXP_CONTAINS(a, '^abc.*')` should be rewritten as `a LIKE 'abc%'`
*   `REGEXP_CONTAINS(a, '.*abc$')` should be rewritten as `a LIKE '%abc'`
*   `REGEXP_CONTAINS(a, '.*abc.*def.*')` should be rewritten as `a LIKE
    '%abc%def%'`

Example of where `REGEXP_CONTAINS` should not be rewritten:

*   `REGEXP_CONTAINS(a, 'abc')` should not be rewritten as `a LIKE '%abc%'`
    since the query does not contain any wildcards.

### In a WHERE clause, use a BETWEEN expression instead of the EXTRACT function

If the column is a `DATE` type: rewrite filter expressions of the form
`EXTRACT(YEAR FROM date_column) = 2024` to `date_column BETWEEN '2024-01-01' AND
'2024-12-31'`.

If the column is a `TIMESTAMP` type: rewrite filter expressions of the form
`EXTRACT(YEAR FROM timestamp_column) = 2024` to `timestamp_column >= '2024-01-01
00:00:00' AND timestamp_column < '2025-01-01 00:00:00'`.

Rationale: BigQuery can statically evaluate `BETWEEN` expressions; it cannot
statically evaluate functions applied to columns.

Example of a query with a date filter expression:

```sql
SELECT
  t1.a0
FROM
  test.a AS t1
WHERE
  EXTRACT(YEAR FROM t1.a2) = 2024;
```

Here is the query rewritten with a date filter expression:

```sql
SELECT
  t1.a0
FROM
  test.a AS t1
WHERE
  t1.a2 BETWEEN '2024-01-01' AND '2024-12-31';
```

### Add a LIMIT clause to an ORDER BY clause

If a query has an `ORDER BY` clause without a `LIMIT` clause, suggest adding a
`LIMIT` clause.

Rationale: In vast majority of cases, users do not need the full results of an
`ORDER BY` clause; `LIMIT` improves the performance of the `ORDER BY` clause.

Example of a query with an `ORDER BY` clause without a `LIMIT` clause:

```sql
SELECT
  t1.a0
FROM
  test.a AS t1
ORDER BY
  t1.a2;
```

Here is the example query rewritten with a `LIMIT` clause:

```sql
SELECT
  t1.a0
FROM
  test.a AS t1
ORDER BY
  t1.a2
LIMIT
  1000;
```

### Remove an ORDER BY clause from a CREATE TABLE statement

If a query is a `CREATE TABLE` or `CREATE OR REPLACE TABLE` statement, remove an
`ORDER BY` clause if it has one.

Rationale: When writing to a table, BigQuery will order the data as needed,
making the `ORDER BY` unnecessary.

Here is an example of a query with a `CREATE OR REPLACE TABLE` statement with an
`ORDER BY` clause:

```sql
CREATE OR REPLACE TABLE test.a AS (
  SELECT
    t1.a0
  FROM
    test.b AS t1
  ORDER BY t1.a2
);
```

Here is the query rewritten with the `ORDER BY` clause removed:

```sql
CREATE OR REPLACE TABLE test.a AS (
  SELECT
    t1.a0
  FROM
    test.b AS t1
);
```

### Compare date columns to DATE literals, not string casts

Rewrite filter expressions of the form `CAST(date_column AS STRING) =
'2025-01-15'` to `date_column = DATE('2025-01-15')`.

Rationale: BigQuery can statically evaluate comparisons against the column
unless the column has a function applied to it.

Example of a query with a date filter expression that casts a `DATE` column to a
string:

```sql
SELECT
  t1.a0
FROM
  test.a AS t1
WHERE
  CAST(DATE(date_column) AS STRING) = '2025-01-15';
```

Here is the query rewritten with a date filter expression that does not cast the
`DATE` column but instead casts the string literal to a `DATE`:

```sql
SELECT
  t1.a0
FROM
  test.a AS t1
WHERE
  date_column = DATE('2025-01-15');
```

### Add a null filter to a NOT IN subquery

Rewrite subqueries within `NOT IN` clauses to add a `WHERE` clause that excludes
nulls.

Rationale: It allows BigQuery to use the more performant anti-hash join instead
of the slower left outer hash join.

Here is an example of a query with a subquery in a `NOT IN` clause:

```sql
SELECT
  t1.a0
FROM
  test.a AS t1
WHERE
  t1.a2 NOT IN (SELECT t2.a2 FROM test.b AS t2);
```

Here is the rewritten query that adds a `WHERE` clause to exclude nulls in the
`NOT IN` subquery:

```sql
SELECT
  t1.a0
FROM
  test.a AS t1
WHERE
  t1.a2 NOT IN (SELECT t2.a2 FROM test.b AS t2 WHERE t2.a2 IS NOT NULL);
```

### Replace a WHERE IN OR IN clause with LEFT JOINs

If the query contains a `WHERE ... IN (...) OR ... IN (...)` clause, which is an
expression that follows this pattern: `columnA IN (subqueryX) OR columnB in
(subqueryY)`, then rewrite the query to use `LEFT JOIN` clauses.

Rationale: `WHERE ... IN (...) OR ... IN (...)` uses a cross product join, which
can be significantly more expensive than `LEFT JOIN`s.

Example of a query with a `WHERE` clause that has two `IN` clauses:

```sql
SELECT
  t1.a0,
  t1.a1
FROM
  test.a AS t1
WHERE
  t1.a2 IN (SELECT t2.a2 FROM test.b AS t2)
  OR t1.a3 IN (SELECT t3.a3 FROM test.c AS t3);
```

Here is the rewritten query with `LEFT JOIN` clauses:

```sql
SELECT
  t1.a0,
  t1.a1
FROM
  test.a AS t1
LEFT JOIN (SELECT DISTINCT a2 FROM test.b) AS t2
  ON t1.a2 = t2.a2
LEFT JOIN (SELECT DISTINCT a3 FROM test.c) AS t3
  ON t1.a3 = t3.a3
WHERE
  t2.a2 IS NOT NULL OR t3.a3 IS NOT NULL;
```

### Replace exact aggregates with approximate aggregates

If the query contains the expression `COUNT(DISTINCT ...)`, suggest rewriting to
use `APPROX_COUNT_DISTINCT(...)`.

This changes exact counts to approximate ones. Propose it and wait for the user
to confirm before applying; do not include it in an otherwise-automatic rewrite.

Rationale: Approximate aggregates are significantly more efficient than exact
aggregates, however at the cost of precision. As a general rule of thumb, large
aggregations should use approximate aggregates, and small aggregations should
use exact aggregates.

Here is an example of a query with a `COUNT(DISTINCT ...)` expression:

```sql
SELECT
  COUNT(DISTINCT t1.a0)
FROM
  test.a AS t1;
```

Here is the rewritten query with `APPROX_COUNT_DISTINCT`:

```sql
SELECT
  APPROX_COUNT_DISTINCT(t1.a0)
FROM
  test.a AS t1;
```

## SQL optimization rules for reducing amount of data

### Replace `SELECT *` with a specific column list

If the query contains a `SELECT *`, tell the user to rewrite to select only the
columns needed. Only suggest this for the top-level `SELECT` in the main query
or in `CREATE TEMP TABLE` statements. Do not suggest this for subqueries in
`WITH` and `WHERE` clauses.

Rationale: Selecting only the necessary columns reduces the amount of data that
BigQuery needs to read and process. `SELECT *` in subqueries do not necessarily
increase the amount of data read, as BigQuery can prune the columns in the
subquery.

Example of a query with a top-level `SELECT *` for which the user should specify
a column list:

```sql
SELECT
  *
FROM
  test;
```

Example of a query with a subquery that has `SELECT *`, which should not need to
be rewritten:

```sql
SELECT
  orders.order_id,
  SUM(orders.quantity) as total_quantity
FROM
  orders
WHERE EXISTS (
    SELECT *
    FROM line_items
    WHERE orders.order_id = line_items.order_id
  )
GROUP BY 1;
```

### Check the query filters on the partitioned column

Check the given query on how it filters (either a `WHERE` clause in a `SELECT`
statement or `ON` clause in a `JOIN` statement) on the partitioned column.

Steps to follow:

1.  Find all referred tables in `SELECT` or `JOIN` statements. If the query does
    not specify the project ID, use the project ID from the context.

    Example 1 where the referred tables follow the format `DATASET.TABLE` and do
    not specify the project ID:

    ```sql
    SELECT
      COUNT(o_orderkey)
    FROM
      my_dataset.orders
    INNER JOIN
      my_dataset.customers
      ON (o_custkey = c_custkey)
    ```

    Assuming that the project ID from the context is `my-project-id`, the
    referred tables in Example 1 are:

    -   `my-project-id.my_dataset.orders`
    -   `my-project-id.my_dataset.customers`

    Example 2 where the referred tables follow the format
    `PROJECT_ID.DATASET.TABLE`:

    ```sql
    SELECT
      COUNT(o_orderkey)
    FROM
      `my-project-id.my_dataset.orders`
    INNER JOIN
      `my-project-id.my_dataset.customers`
      ON (o_custkey = c_custkey)
    ```

    The referred tables in Example 2 are:

    -   `my-project-id.my_dataset.orders`
    -   `my-project-id.my_dataset.customers`

2.  Fetch the table schema from BigQuery for each referred table and get the
    partitioned column for each table if it exists.

    In the JSON output, use the field
    `partitionDefinition.partitionedColumn.field`. Note that this field is
    repeated, but in practice only one field is populated when the table is
    partitioned.

3.  For each table with a partitioned column, check the following rules:

    a. Ensure that the query contains a filter condition for the partitioned
    column for each table reference. If there is no filter condition for the
    partitioned column and the table has a partitioned column, tell the user.

    Rationale: If a table is partitioned, queries should leverage the
    partitioning to reduce the amount of data to read.

    Note that if the table's schema specifies that the partition filter is
    required (as specified in the field `requirePartitionFilter`), then no
    filter condition will cause the query to return an error.

    Example of query that does *not* have a filter condition on a partitioned
    column `o_orderdate` for table reference `my_dataset.orders`:

    ```sql
    SELECT
      COUNT(o_orderkey)
    FROM
      my_dataset.orders
    INNER JOIN
      my_dataset.customers
      ON (o_custkey = c_custkey)
    ```

    Example suggested improvement to show users:

    ```sql
    SELECT
      COUNT(o_orderkey)
    FROM
      my_dataset.orders
    INNER JOIN
      my_dataset.customers
      ON (o_custkey = c_custkey)
    WHERE
      -- Ask the user to provide the start and end dates.
      o_orderdate BETWEEN @start_date AND @end_date
    ```

    b. If the filter condition for the partitioned column is dynamic
    (specifically, a scalar subquery or a column-dependent aggregation), rewrite
    the query so that the filter condition is in a variable.

    Example of a query with a dynamic filter condition that is doing an
    aggregation on a partitioned column:

    ```sql
    SELECT o_orderstatus, o_orderdate
      FROM `my_dataset.orders`
    WHERE
      o_orderdate = (
        SELECT max(o_orderdate) FROM `my_dataset.orders`
      );
    ```

    Here is the query rewritten so that the filter condition is in a variable:

    ```sql
    DECLARE max_order_date DATE DEFAULT (
      SELECT max(o_orderdate) FROM `my_dataset.orders`
    );
    SELECT o_orderstatus, o_orderdate
    FROM `my_dataset.orders`
    WHERE
      o_orderdate = max_order_date;
    ```

    Note that the variable `max_order_date` still scans the entire table,
    however the full scan is still more efficient than the original query. If
    the table is configured with `require_partition_filter = true`, then the
    user needs to add an additional filter condition within the variable
    `max_order_date`.

#### Fetching table schemas from BigQuery

Use the following shell command to fetch the table schema from BigQuery:

-   If the query specifies the project ID:

    ```bash
    bq show --format=prettyjson PROJECT_ID:DATASET.TABLE
    ```

    For example, given this table reference: `my_project:my_dataset.orders`, run
    the command:

    ```bash
    bq show --format=prettyjson my_project:my_dataset.orders
    ```

-   If the query does not specify the project ID:

    ```bash
    bq show --format=prettyjson DATASET.TABLE
    ```

    For example, given this table reference: `my_dataset.orders`, run the
    command:

    ```bash
    bq show --format=prettyjson my_dataset.orders
    ```

