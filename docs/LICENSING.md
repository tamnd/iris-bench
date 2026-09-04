# Corpora, licensing and fair use

Everything this repository measures is measured on data that belongs to someone else. This page says what we do with each category and why.

## Three categories

**Generate.** The corpus is produced locally from a generator whose output is deterministic given a seed and a scale factor. Nothing is redistributed. TPC-H and TPC-DS are here, as is the Star Schema Benchmark.

**Fetch.** The corpus is downloaded from its canonical location at run time, verified against a recorded digest, and never redistributed by us. ClickBench, the Join Order Benchmark's IMDB dump, Public BI, and the h2o benchmark data are here.

**Mirror.** The licence permits redistribution, and mirroring is the difference between a reproducible result and a broken link in three years. Silesia and enwik8 and enwik9 are here. Every mirrored corpus carries a licence note in its manifest, and CI fails if one is missing.

The default is fetch. Mirroring requires reading the licence and writing down what it permits.

## TPC workloads

TPC-H and TPC-DS are trademarks of the Transaction Processing Performance Council, and the rules for using them outside an audited benchmark run are specific.

What we do:

- Results are described as derived from the TPC-H or TPC-DS specifications. They are never described as TPC-H or TPC-DS results.
- The trademark attribution appears on every page carrying a number from either workload.
- Results are never compared against an official TPC Result. That is prohibited by TPC and by us.
- Scale factors that are not compliant scale factors are marked as a deviation in the row, so a reader can see it without reading a footnote.
- A skewed generator is a different benchmark with a different name, never merged into the TPC-H numbers.

The distinction that matters: an unaudited run of a query set derived from a public specification is a legitimate and common thing to publish. Calling it a TPC-H Result is not. The two are one sentence apart and the sentence is load bearing.

## ClickBench

ClickBench's data and query set are published by ClickHouse under the Apache License 2.0. The benchmark's own rules are followed exactly when running under its protocol: all 43 queries, three runs, the second and third used, the geometric mean reported, and the combined score computed the way the leaderboard computes it, from load time, storage size, cold runs and hot runs.

Running a subset and calling it ClickBench is not permitted here even though nobody would stop us. All 43 queries run or the run is invalid.

## The Join Order Benchmark

The IMDB data underneath it comes from IMDB's non-commercial datasets. It is fetched, never mirrored, and the queries themselves are from the published artifact.

## Compression corpora

Silesia is public domain for benchmarking purposes and is mirrored. The enwik8 and enwik9 files come from a Wikipedia dump and inherit its licence, which permits redistribution with attribution, so they are mirrored with the attribution in the manifest.

## Public BI

The Public BI benchmark's data comes from Tableau Public workbooks. It is fetched from the published location and not redistributed. The 36 dataset subset used by the encoding literature is defined as a subset object with its own digest, so that a result labelled with that subset is unambiguous about which 36.

## Publishing

Three rules that go beyond what any licence requires.

Numbers that make a system look bad go to that system's maintainers before they go on the site, with the reproduction bundle attached and a two week window to tell us we configured them wrong. Their response, or the absence of one, is recorded next to the number. Most results of the form "system X is slow" are configuration errors, and the people who wrote system X spot them in minutes.

A reproduction that fails is a claim about someone's work made by strangers on hardware they never had. The authors are contacted before a failed verdict is published, every time. The usual outcome of asking is that the reproduction then succeeds.

An artifact that cannot be run is recorded as not attemptable with the reason, and the reason is worded as a fact about our circumstances rather than as a criticism of theirs.
