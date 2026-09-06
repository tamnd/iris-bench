# iris-bench

Reproducible benchmarks for columnar storage formats, and a standing attempt to re-run the numbers the papers published.

The field has a measurement problem. BtrBlocks reports 2.2 times faster than Parquet, DuckDB reports on par, AnyBlox reports a 5% sandbox tax, and LiquidCache reports that decoding is the primary bottleneck. None of those contradict each other. They are answers to different questions that happen to share the word "faster", taken on different hardware, against baselines whose configuration was almost never disclosed. This repository exists to take the same measurements the same way, publish the tuple rather than the number, and say so when a result does not reproduce.

It is the measurement half of [`tamnd/iris`](https://github.com/tamnd/iris), and it is deliberately a separate repository, because a project that grades its own homework should at least keep the marking scheme somewhere else. Nothing here privileges `iris`. It gets no visual distinction on the results pages, no separate configuration path, and it appears in the tables alphabetically like everything else.

## The three rules

**A result is a tuple, not a number.** System, version, configuration digest, corpus, corpus digest, workload, query, storage tier, machine class, protocol, repetition, and the measurement. Producing a bare "2.2 times faster" requires actively discarding fields, which is the point of the schema.

**Baselines are configured by their own documentation.** Every system in the matrix carries a `CONFIG.md` naming the source of its configuration, the settings applied, the deviations and their reasons, the alternatives that were rejected, and the date the configuration was last reviewed. Where we had to guess, the row records that we guessed, and every number produced that way renders with a marker.

**Losses are published.** If `iris` is slower, the number goes up. If a reproduction fails, the verdict goes up, after contacting the authors. If a rival's maintainer says we configured them wrong, that goes up too, alongside the corrected run, with the original struck through rather than deleted.

## What is in scope

Query benchmarks: ClickBench, TPC-H, TPC-DS, the Star Schema Benchmark, the Join Order Benchmark, and the h2o database benchmark. Corpora: Public BI, the ALP double columns, the FSST text corpora, Silesia, and enwik8 and enwik9. Formats: Parquet, ORC, Arrow IPC, BtrBlocks, FastLanes, Vortex, Lance, Nimble, and the self-decoding formats. Engines: DuckDB, DataFusion, ClickHouse, Polars.

Five storage tiers, from a warm in-process buffer to real object storage over a real network, because a ranking taken on a warm local file is a ranking of one tier and the field mostly publishes that one.

## What is out of scope

This is not a general purpose benchmarking framework. It measures columnar storage formats and the systems that read them, and it stops there.

## The original contribution

Public BI has been both the design input and the evaluation set for six years of encoding research. The formats that do well on it were, in part, designed against it. So `iris-bench` assembles a held-out corpus, publishes the selection criteria before running anything against it, freezes it by content digest before any decoder is tuned, and reports the delta per format between Public BI and the held-out set. A format that degrades least on unseen data is not thereby better, and the write-up says so, but the delta is a measurement nobody in this area has published.

## Status

Pre-alpha. B0 is done as of `v0.1.0`, so the measuring apparatus exists and the benchmarks do not. Milestones B0 through B8 are [public](https://github.com/tamnd/iris-bench/milestones) with one issue per exit gate.

What B0 established is in `docs/ROADMAP.md` under that milestone, and the short version is that this fleet can measure, on one machine, for ratios always and for durations under a gate. The noise floor is 1.05% on the one eligible role and over two percent everywhere else, and the harness adds 41.1 ns to a sample, which is under one percent of anything longer than 4.1 microseconds. `iris-bench check`, `noise`, `overhead`, `resident` and `corpus` run today. Nothing else does.

B1 is in progress and is the corpora. ClickBench, TPC-H at scale factor 1 and 20, Public BI in both the full 206 table set and the 36 table subset, and Silesia and enwik8 are pinned and reproduce, some by download, the TPC-H pair from `dbgen`, and the last two from a mirror because their original hosting has moved more than once.

B0 through B4 need no `iris` code to exist, which is deliberate. If `iris` is never built, the reproduction of the published figures and the storage tier study still stand on their own.

## Layout

| Crate | What it is |
|---|---|
| `bench-core` | Timing, repetition, bootstrap confidence intervals, the result row. No benchmarks in it. |
| `bench-env` | Environment capture and the eligibility gates that abort a run on an unfit machine. |
| `bench-corpus` | Corpus manifests, fetching, generating, verifying, and the content addressed store. |
| `bench-driver` | The `Driver` trait every system implements. The prepare, load and run split lives here. |
| `bench-run` | The runner. Process isolation, ordering, storage tiers, cache control. |
| `bench-store` | Append only result storage and the claim ledger. |
| `bench-report` | Rendering, including the rules that stop a misleading table being drawn. |
| `bench-cli` | `iris-bench` the command line tool. |
| `drivers/*` | One crate per system under test. They depend on `bench-driver` and nothing else in the tree. |

## Running it

Requires Rust 1.98 or newer.

```
cargo build --workspace
cargo test --workspace
```

The harness refuses to produce a publishable number on a machine that fails the eligibility gates, and it says which gate failed. That is not configurable.

Getting a corpus onto the machine is one command, and it is worth pointing the store somewhere with room because ClickBench alone is 13.8 GiB.

```
cargo run -p iris-bench-cli -- corpus clickbench-hits --store /var/tmp/iris-corpus
```

The digest is checked in the same pass that writes the file, so bytes that are not what the manifest promised never land in the store. The asserted row and column counts are checked after, which is what catches a download that stopped early and still parses. `docs/CORPORA.md` is the format.

A generated corpus is the same command with the generator pointed at. Nothing is downloaded and nothing is redistributed, and the tool checks that the generator is the pinned version before it produces a byte, because a different generator writes different bytes and a digest mismatch on its own does not say which of those two things went wrong.

```
cargo run -p iris-bench-cli -- corpus tpch-sf1 --generator ~/tpch-kit/dbgen/dbgen --store /var/tmp/iris-corpus
```

## Machines

Results are labelled by hardware class rather than by machine name. `docs/MACHINES.md` lists the fleet, what each class is used for, and what it cannot be used for.

## Licence

Apache License 2.0. See `LICENSE`.

## Trademarks

TPC-H and TPC-DS are trademarks of the Transaction Processing Performance Council. Results published here are derived from the TPC-H and TPC-DS specifications, are not audited, and are not comparable with official TPC Results. See `docs/LICENSING.md`.
