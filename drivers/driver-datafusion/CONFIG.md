# driver-datafusion configuration

## Source

DataFusion's own benchmark harness in `apache/datafusion`, under `benchmarks/`, which is what its maintainers publish numbers from, together with the library documentation for `SessionConfig` and `RuntimeEnv`. The ClickBench entry there registers the Parquet files where they lie and queries them in place, and the TPC-H entry does the same over converted Parquet.

The version under test is fixed by this workspace's lockfile, since DataFusion is a Rust library compiled into this binary rather than a server somebody installed. The driver still asks the running system what version it is, through DataFusion's own `version()` function, rather than reporting a constant from build metadata, so a build that somehow linked something else says so.

## Settings

`target_partitions` is set from the machine class, not left at DataFusion's default. The default reads the host's core count, and a default that reads the host makes every result a result about that host rather than about DataFusion.

The tokio runtime is built with the same thread count for the same reason. DataFusion runs its plan on whatever runtime it is called from, so a default multi threaded runtime would size itself from the host as well and the partition count would be the only half of the parallelism anybody controlled.

`RuntimeEnv` is given a memory limit from the machine class. DataFusion's default is an unbounded pool, which is a reasonable default for a library and a terrible experimental control, and it also means a query that would spill on a fleet machine instead grows until the host says no. The whole budget goes to the pool rather than a fraction of it, because the budget in `Setup` is already the allowance for the system under test and reserving a further slice here would quietly give DataFusion less than the other drivers got.

The disk manager is pointed at a subdirectory of the run's scratch directory, so spilling happens somewhere known, gets measured as part of the run, and is removed with everything else afterwards.

Tables are registered as listing tables over the exact files the corpus manifest names, one url per file, rather than over a directory. A directory would be read as whatever happens to be on the disk at the time, and the manifest is the thing that says what a corpus is.

Where the workload supplies a select list, the listing table is registered under the table's name with `_raw` on the end and the workload's name becomes a view over it. That is the shape of DataFusion's published ClickBench setup, which registers `hits_raw` and creates `hits` as `SELECT * EXCEPT ("EventDate"), CAST(CAST("EventDate" AS INTEGER) AS DATE) AS "EventDate" FROM hits_raw`. The corpus stores `EventDate` as an unsigned sixteen bit count of days, so without the cast the queries that filter on a date compare a number against a string. `bench-workload` carries the setup script with its digest and hands the same select list to every driver that gets one.

That entry converts `EventDate` and nothing else. `EventTime`, `ClientEventTime` and `LocalEventTime` stay as the raw Unix seconds the file holds, which is why DataFusion's published query file writes `to_timestamp_seconds("EventTime")` by hand where the ClickHouse reference does not, and why DuckDB's published setup converts four columns here and DataFusion's converts one. Making those two agree would be this repository configuring the benchmark rather than running it.

## Deviations

Data is not ingested. DataFusion is a query engine over files rather than a database with its own storage, so the load phase registers the files and infers a schema, and every scan is paid for in the run phase. This is DataFusion's published configuration and it is the honest one, but it means the load number here is not comparable with the load number from a driver that ingested, and the run numbers carry work that the other driver did once. The phases are reported separately for exactly this reason, and a reader comparing only the run column across the two is comparing different things.

There is no in memory alternative worth having. ClickBench `hits` is 14 GB and TPC-H at scale factor 20 is 22.5 GB, neither of which fits in the memory budget of any machine in this fleet, so a driver that materialised into memory would simply fail on the corpora this repository was built to measure.

`binary_as_string` is not set on the Parquet format options, and DataFusion's published setup passes it. On the ClickBench corpus this repository pins, the option changes nothing: every byte array column in that file already carries the `String` logical type and the `UTF8` converted type, which was checked by reading the file's footer rather than assumed. It is worth revisiting if a corpus ever arrives whose string columns lack those annotations, because then it would be the difference between reading a column as text and reading it as bytes.

The view is created with a paid conversion on every scan rather than materialised into a table. Materialising would move that work into the load phase, and the whole reason DataFusion's load number is small and its run numbers carry the scan is that this is what its published entry does. Turning it into the DuckDB shape would hide the difference the phases exist to show.

Text corpora are read with DataFusion's schema inference and no header row, so the column names and types come from DataFusion rather than from the workload. That is the same gap the DuckDB driver has, two systems could disagree about a result because they inferred a column differently rather than because they computed differently, and it closes when the workloads land with their own schemas.

## Rejected alternatives

Writing the corpus to Parquet first and registering that, which is what DataFusion's TPC-H harness does with the `dbgen` output, was rejected. The conversion is not part of any phase this harness times, so it would be work done off the clock, and the corpus is pinned by digest as the bytes that were published rather than as something this repository rewrote.

Setting `batch_size` away from its default was rejected. It is a real tuning knob and DataFusion's own entries leave it alone, so changing it would be this repository tuning a baseline by its own guesswork, which is the thing the `CONFIG.md` rule exists to stop.

Enabling `repartition_file_scans` or the other repartition switches by hand was rejected for the same reason. They are on by default already, and a driver that turned defaults on and off by hand would be publishing a configuration DataFusion's maintainers never put forward.

## Last reviewed

2026-09-06
