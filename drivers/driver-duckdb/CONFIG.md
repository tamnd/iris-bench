# driver-duckdb configuration

## Source

DuckDB's own performance guide, at `duckdb.org/docs/stable/guides/performance`, read for this driver rather than reconstructed from what other benchmark harnesses do. The pages that matter are the environment page for threads and memory, the "how to tune workloads" page for insertion order, and the import page for how data is meant to get in.

DuckDB is linked in bundled, so the version under test is fixed by this workspace's lockfile rather than by whatever each machine had installed. A version that varies by machine is a version nobody pinned, and it appears in every result row. The driver still asks the running system what version it is rather than reporting a constant, so a build that somehow linked something else says so.

## Settings

`threads` is set from the machine class, not left at DuckDB's default. The default reads the host's core count, and a default that reads the host makes every result a result about that host rather than about DuckDB.

`memory_limit` is set from the machine class for the same reason. DuckDB's default is 80% of the machine's RAM, which is a sensible default and a terrible experimental control.

`temp_directory` is set to a subdirectory of the run's scratch directory, so that spilling happens somewhere known, gets measured as part of the run, and is removed with everything else afterwards. Leaving it at the default puts spill files next to the database, which is also inside the scratch directory, but only by accident rather than by decision.

`preserve_insertion_order` is set to false. This is in DuckDB's own performance guide as the first thing to change for large imports, and it lowers memory use on load. It means a query without an `ORDER BY` may come back in a different order, which costs nothing here because the canonical result form sorts rows that the query did not order.

Data is loaded with `CREATE OR REPLACE TABLE ... AS SELECT * FROM read_parquet(...)` or `read_csv(...)`, into a persistent database file rather than an in-memory one. That matches DuckDB's published ClickBench entry and the import guidance, and it is also a configuration somebody actually deploys.

## Deviations

Reading the files in place, as a view over `read_parquet`, would be a legitimate thing to measure and is not what happens here. It moves the work from the load phase into the run phase, and DuckDB's own published entry loads into a table, so a view would be measuring a configuration DuckDB does not put forward. The load timing is where this choice is visible, which is the reason that phase is timed separately.

`DECIMAL` results are rendered as doubles. DuckDB computes TPC-H money aggregates in exact decimal arithmetic and the other systems in the matrix return the same aggregate as a double, so an exact rendering would be a comparison only one of the three could pass. The canonical form rounds floats to six significant digits anyway, which is well inside the spread of a decimal and a double summing the same column. This is a deviation from what DuckDB actually computed and it is recorded as one.

CSV and pipe separated files are loaded with DuckDB's schema auto-detection rather than an explicit column list. That means the column types come from DuckDB's sniffer and not from the workload, and two systems could disagree about a result because they inferred a column differently rather than because they computed differently. This is a real gap and it closes when the workloads land with their own schemas.

## Rejected alternatives

Linking against a system libduckdb was rejected. It would make the build much faster, and it would also mean the version under test is whatever each fleet machine happened to have, with a mismatch showing up as a silently different number rather than as an error.

Setting `threads` to the physical core count, which is what the performance guide suggests, was rejected in favour of taking it from the machine class. The guide is written for someone tuning a deployment, where the host is the point. Here the host is the thing being controlled for, and every system in the matrix has to be given the same thread count or the comparison is between thread counts.

Tuning anything not named in DuckDB's own documentation was rejected outright. A setting somebody here invented is a setting DuckDB's maintainers never endorsed, and the whole point of the `CONFIG.md` rule is that a baseline is configured by its own documentation.

## Last reviewed

2026-09-06
