# Contributing to iris-bench

Thanks for looking. The most valuable contribution to a benchmark repository is usually not code. It is telling us we measured something wrong.

## The three things we most want

**Tell us a system is misconfigured.** Every system under test has a `CONFIG.md` naming where its configuration came from. If you maintain one of those systems, or you just know it well, and the configuration is wrong or out of date, open an issue. This is not a formality. Most results of the form "system X is slow" are configuration errors, and the people who wrote system X spot them in minutes.

**Tell us a published number does not reproduce.** Every result links to a reproduction bundle that is meant to be complete enough for a stranger to run. If you run one and get a different answer, that is our bug, tracked and fixed like any other, and it gets recorded as a correction whether or not we agree with you.

**Add a system or a format to the matrix.** A driver lives in `drivers/` and depends on `bench-driver` and nothing else in the tree. The bar is a `CONFIG.md` with a real source, not a set of flags that looked reasonable.

## Rules that are not negotiable

**A driver may not know it is being benchmarked.** No special case for any system, including `iris`. Warm up is the runner's job, the prepare, load and run split is fixed in the trait for everyone, and there is no configuration path that one system has and another does not.

**Configuration comes from the system's own documentation.** If you had to guess, set `config_source` to say you guessed. A guess that is labelled is a limitation. A guess that is not labelled is a misrepresentation.

**Losses are published.** A change that makes a result disappear because it was unflattering will be rejected. Corrections strike through the original rather than replacing it.

**Timing code changes bump the methodology version.** Change point detection resets at that boundary rather than running across it. Skipping the bump produces a false regression in whichever system happened to be measured next, and it is genuinely hard to diagnose after the fact.

## Practical things

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The toolchain is pinned to Rust 1.98 in `rust-toolchain.toml`, which is also the minimum supported version.

CI enforces a set of checks that are about discipline rather than about compilation: every corpus has a manifest with digests, every mirrored corpus has a licence note, every driver has a `CONFIG.md` with all its required fields and a review date, every deviation has a reason string, every driver crate depends only on `bench-driver`, and every confirmatory row references a claim that exists. Those are the checks that make the method real instead of aspirational, so please do not disable one to get a pull request through.

Commits follow [Conventional Commits](https://www.conventionalcommits.org/).

## Adding a corpus

A corpus needs a manifest with its source, its licence, its category of generate or fetch or mirror, the digests of its files, and assertions about its shape such as the row and column counts. The assertions exist so that a corpus that quietly changed upstream fails loudly rather than producing different numbers.

## Reporting a security issue

See `SECURITY.md`. The interesting cases here involve corpus fetching and archive extraction, since this tool downloads and unpacks files from the internet by design.

## Licence

By contributing you agree that your contribution is licensed under the Apache License 2.0.
