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

## Deviations

Data is not ingested. DataFusion is a query engine over files rather than a database with its own storage, so the load phase registers the files and infers a schema, and every scan is paid for in the run phase. This is DataFusion's published configuration and it is the honest one, but it means the load number here is not comparable with the load number from a driver that ingested, and the run numbers carry work that the other driver did once. The phases are reported separately for exactly this reason, and a reader comparing only the run column across the two is comparing different things.

There is no in memory alternative worth having. ClickBench `hits` is 14 GB and TPC-H at scale factor 20 is 22.5 GB, neither of which fits in the memory budget of any machine in this fleet, so a driver that materialised into memory would simply fail on the corpora this repository was built to measure.

Text corpora are read with DataFusion's schema inference and no header row, so the column names and types come from DataFusion rather than from the workload. That is the same gap the DuckDB driver has, two systems could disagree about a result because they inferred a column differently rather than because they computed differently, and it closes when the workloads land with their own schemas.

## Rejected alternatives

Writing the corpus to Parquet first and registering that, which is what DataFusion's TPC-H harness does with the `dbgen` output, was rejected. The conversion is not part of any phase this harness times, so it would be work done off the clock, and the corpus is pinned by digest as the bytes that were published rather than as something this repository rewrote.

Setting `batch_size` away from its default was rejected. It is a real tuning knob and DataFusion's own entries leave it alone, so changing it would be this repository tuning a baseline by its own guesswork, which is the thing the `CONFIG.md` rule exists to stop.

Enabling `repartition_file_scans` or the other repartition switches by hand was rejected for the same reason. They are on by default already, and a driver that turned defaults on and off by hand would be publishing a configuration DataFusion's maintainers never put forward.

## Last reviewed

2026-09-06
