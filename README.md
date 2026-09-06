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

Pre-alpha. B0 is done as of `v0.1.0` and B1 as of `v0.2.0`, so the measuring apparatus and the data exist and the benchmarks do not. Milestones B0 through B8 are [public](https://github.com/tamnd/iris-bench/milestones) with one issue per exit gate.

What B0 established is in `docs/ROADMAP.md` under that milestone, and the short version is that this fleet can measure, on one machine, for ratios always and for durations under a gate. The noise floor is 1.05% on the one eligible role and over two percent everywhere else, and the harness adds 41.1 ns to a sample, which is under one percent of anything longer than 4.1 microseconds. `iris-bench check`, `noise`, `overhead`, `resident`, `corpus`, `clickbench` and `reproduce` run today. Nothing else does.

B1 is done and is the corpora. ClickBench, TPC-H at scale factor 1 and 20, Public BI in both the full 206 table set and the 36 table subset, and Silesia and enwik8 are pinned and reproduce, some by download, the TPC-H pair from `dbgen`, and the last two from a mirror because their original hosting has moved more than once. Every one of them was fetched or generated end to end through the real command rather than checked on paper, which is 43 GB of Public BI and 22 GB of TPC-H among other things.

A digest mismatch is a hard failure with no override, and the absence of an override is enforced by a check that reads the four files a corpus's bytes pass through rather than left as an intention. Generated corpora are pinned to the platforms they have actually been produced on, and generating anywhere else is refused with a reason instead of attempted.

B0 through B4 need no `iris` code to exist, which is deliberate. If `iris` is never built, the reproduction of the published figures and the storage tier study still stand on their own.

## Layout

| Crate | What it is |
|---|---|
| `bench-core` | Timing, repetition, bootstrap confidence intervals, the result row. No benchmarks in it. |
| `bench-env` | Environment capture and the eligibility gates that abort a run on an unfit machine. |
| `bench-corpus` | Corpus manifests, fetching, generating, verifying, and the content addressed store. |
| `bench-driver` | The `Driver` trait every system implements. The prepare, load and run split lives here. |
| `bench-run` | The runner. Process isolation, ordering, storage tiers, cache control. |
| `bench-workload` | The published workloads, carried verbatim, and the rules their authors run them under. |
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

Running ClickBench is one command per system. The 43 queries are the published files carried here byte for byte, three runs each, first run reported cold and best of the rest reported hot, which is the protocol the upstream harness uses.

```
cargo run --release -p iris-bench-cli -- clickbench run --driver duckdb --file /var/tmp/iris-corpus/hits.parquet --out duckdb.json
```

Each system also gets the setup its own entry publishes, not just the queries. The corpus stores `EventDate` as a count of days and three of its timestamps as plain Unix seconds, so every published entry converts those columns on the way in, and the two entries here do not convert the same ones or pay for it at the same point. DuckDB materialises four of them into a table while it loads, and DataFusion converts one in a view over the files and leaves the rest to its queries. Both scripts are carried with their address and their digest for the same reason the queries are, and neither is corrected to look like the other.

The order the queries are visited in is randomised and the seed is written into the record, so handing that seed back replays the same schedule. Dropping the page cache before each query is `--cold` and it needs root, and a run that could not drop it records why rather than calling itself cold anyway. The tool refuses to measure on a machine that failed the eligibility gates unless `--anyway` is passed, since a number taken on an unfit machine is a number somebody will put in a table.

Then the records are compared, which is the part that catches a system being fast because it answered a different question.

```
cargo run --release -p iris-bench-cli -- clickbench check duckdb.json datafusion.json arrow-parquet.json
```

Agreement is per query, across every system that answered it. A query only one system answered is reported separately rather than counted as confirmed, and a system that gave two different answers across its own three runs is named even when the systems agreed with each other.

Ten of the forty three do not have one answer, and they are named in code with the reason attached rather than tolerated when they come up. Nine put a LIMIT on top of an ordering that does not tell the rows inside the window apart from the ones just outside, so two correct systems return a different arbitrary ten. The tenth asks for every column, and the two setups ClickBench publishes do not build the same table out of the same corpus, so the same rows cannot render the same way. Each of the ten was run against the real corpus and read before it went on the list, a test pins the list so it cannot grow quietly, and everything not on it still has to agree.

A digest tells you two systems differ and tells you nothing about what they differ on, so there is a fourth command for the moment after a comparison fails. It takes the table in the same way a run does, runs only the queries you name, and prints the rows both digests were taken over.

```
cargo run --release -p iris-bench-cli -- clickbench answer --driver duckdb --file /var/tmp/iris-corpus/hits.parquet --query q23
```

It takes no timings, so it needs no gate and it is fine on a busy machine, and nothing it prints can end up in a table of numbers.

The other check on the same records is against the public leaderboard, which is what says the harness itself is not wrong.

```
cargo run --release -p iris-bench-cli -- clickbench calibrate duckdb.json datafusion.json arrow-parquet.json
```

ClickBench publishes the numbers behind every row of its leaderboard, and this carries the DuckDB and DataFusion files for c6a.4xlarge with their address and their digest. The machine here is not that machine, so the comparison is not against any particular ratio. A faster machine moves every query on every system by about one factor, so each driver gets its own ratio against the published numbers, the machine factor is the geometric mean of those ratios, and a driver more than 25 percent away from the shared factor is reported as misconfigured along with the queries that put it there. What that cannot catch is a mistake that slows everything here equally, which looks exactly like a slower machine, and the crate says so rather than leaving it to be discovered.

All of that rests on the run being a fair measurement of the machine it was taken on, so the record now carries whether the machine passed its gates, what overrode them when it did not, and how many threads and how much memory each system was given. Calibration refuses to grade a run that did not pass, because a busy machine is not one factor the arithmetic divides out: another tenant competing for memory bandwidth slows the queries that stream and leaves the rest alone, which is the same shape a misconfigured driver makes. It also refuses records that were given different budgets, since a shared machine factor across two drivers means something only if both ran on the same machine. Passing `--anyway` prints the table with all of that said on it, which is for looking rather than for publishing.

## Claims

A claim this project intends to make is registered before it is run, with the threshold that decides it, and `docs/CLAIMS.md` is that ledger. The registrations are committed files under `crates/bench-store/claims/`, one per claim, and running one is one command.

```
cargo run --release -p iris-bench-cli -- reproduce C0002
```

It emits one of five words and there is no sixth: REPRODUCED, REPRODUCED-WITH-CAVEAT, NOT-REPRODUCED, NOT-ATTEMPTABLE, PENDING. The three that follow from a measurement are arithmetic over the reading, the threshold and what the instrument can resolve, rather than a word somebody picks after seeing the number. A reading outside the bar is NOT-REPRODUCED whatever else was true about the run, and the only thing a caveat does is turn a pass into a pass with the condition next to it.

NOT-ATTEMPTABLE covers two situations and both are facts about our circumstances rather than criticism of anybody's work. One is an artifact that cannot be obtained or cannot be run here, which has to carry a citation saying where it was looked for. The other is a bar narrower than what our instrument can resolve, and it is why a claim settled by a comparison runs its own control in the same session. A harness that cannot put two copies of one buffer closer than nine percent has no business reporting that two different things are three percent apart, so a reading that close to its bar is recorded as undecided and the bar is not widened to a number the instrument happens to clear.

Unlike the gates this exits zero on every verdict, because a failed reproduction is a result. What it refuses is a claim nobody registered, a reading taken before the day its own threshold was written down, and a claim whose instrument nobody has built yet, since work that was never attempted is a gap here rather than a verdict.

## Machines

Results are labelled by hardware class rather than by machine name. `docs/MACHINES.md` lists the fleet, what each class is used for, and what it cannot be used for.

## Licence

Apache License 2.0. See `LICENSE`.

## Trademarks

TPC-H and TPC-DS are trademarks of the Transaction Processing Performance Council. Results published here are derived from the TPC-H and TPC-DS specifications, are not audited, and are not comparable with official TPC Results. See `docs/LICENSING.md`.
