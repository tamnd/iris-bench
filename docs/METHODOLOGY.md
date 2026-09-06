# Methodology

The short version of how a measurement is taken here, and why each choice was made. It leans on three papers: Kalibera and Jones on repetition levels, Mytkowicz and colleagues on measurement bias, and Raasveldt and colleagues on fair benchmarking.

## Before anything runs

The machine has to pass its eligibility gates. Frequency scaling, turbo, the state of other tenants, thermal headroom, available memory, and whether anything else is running. A gate failure aborts the run and names the gate. It does not produce a slower number with a warning attached, because a warning next to a number gets copied without the warning.

The environment is captured to a file and hashed, and that hash is in every result row. Two rows with different environment hashes were not taken under the same conditions, and the schema makes that visible instead of leaving it to a reader's memory.

## Repetitions

Kalibera and Jones's real contribution is not "run it more times". It is that variance lives at a particular level, and repetitions should be spent at the level that carries it. A pilot run identifies whether the variance is between builds, between process invocations, or between iterations within a process, and the repetition budget goes there. For a released binary, build to build variance is usually negligible, so one build and more iterations is strictly better than three builds and fewer.

The headline statistic is the median with a bootstrap confidence interval. The minimum is a separate column, because the minimum is a statistic about the luckiest run and reporting it as if it were typical is one of the most common quiet distortions in this field.

Repetitions are added on the confidence interval width of a single measurement, never on whether a comparison has become significant. Adaptive stopping keyed on the comparison is how you manufacture a result.

## Measurement bias

Mytkowicz and colleagues showed that changing the link order of a binary or the size of the environment can move a measurement by more than the effect most papers report. That is not a rounding error, it is a systematic bias that survives any number of repetitions.

So for any effect smaller than ten percent, the measurement runs under a varied build protocol: several link orders, several environment sizes, and the effect has to survive all of them. An effect that appears under one link order and not another is a fact about the linker.

Ordering is randomised by default with a recorded seed, so that a re-run with the same seed reproduces the schedule exactly and a re-run with a different seed is an independent check.

## Warm, cold cache, cold start

Three different things that get called the same thing.

Warm means the data is in the page cache and the process has already run the query once. Cold cache means the page cache was dropped and the process is running the query for the first time on that data. Cold start means the process itself has just started, which for a system with an ahead of time compilation cache or a query compiler is a substantially different measurement.

All three are separate metrics. None of them is the default. The tier and the cache state are in the result key.

Cross platform comparison of cold numbers is prohibited, because the primitive for dropping a cache is not equivalent across operating systems and the resulting numbers are not measuring the same thing.

## The three phases

Every system under test is driven through the same three phases, and each one is timed separately. Prepare is starting the system and applying its configuration. Load is getting a table in. Run is answering a query. Conflating them is the most common way a benchmark accidentally measures the wrong thing: load time charged to query time makes a system look slow, and an index built during preparation makes it look fast.

Preparation is not given any file paths. That is the mechanism rather than a convention, because a driver handed the corpus before the load phase could read it, index it, convert it, or cache it, and every one of those would land in the phase the load timing is supposed to account for. Preparation gets a scratch directory, a thread count and a memory budget, and nothing it could start early with.

A query cannot be answered before something has been loaded. A system that could answer one read the data during preparation, and the split exists precisely so that this shows up rather than disappearing into a phase nobody looks at.

The driver does not hold the clock. The harness borrows the driver, calls the three methods itself, and times each call from the outside, so a driver cannot report a preparation that took no time. Drivers are checked in CI for a clock of their own, and a crate under `drivers/` that does not implement the trait fails the build, which is what makes "every driver reports all three" a property rather than an intention.

All three phases are always reported. A driver that reads its files in place has a small load and a large run, one that ingests into a native format has the opposite, and both of those are true things about the system that a single total would hide. Loading can happen many times, because TPC-H is eight tables and ClickBench is one.

A query has to return a materialised result. A system with lazy evaluation that hands back a plan has been timed on building a plan, and comparing that against a system that actually computed the answer is not a comparison.

## What gets compared

Decode loop time, format scan time, and end to end query time are three different measurements and they are stored as three different metrics. Most of the apparent contradictions in the published literature come from comparing one against another. Nobody involved was being dishonest, they were answering different questions that share a word.

Geometric means never appear without the per query table on the same page, and the count of queries that completed appears next to every aggregate. A system that answers 38 of 43 queries must not be able to look like one that answers 43.

Where many comparisons are made at once, the multiplicity is accounted for, because 43 queries and a five percent threshold will produce a couple of exciting results from pure noise.

## Correctness first

Every query produces a digest of its canonicalised result, and the digests are compared across systems. A row whose result does not match is marked incorrect and its timings are excluded from every aggregate.

This is not paranoia. The motivating example in the fair benchmarking literature is a prototype that was faster because it used hardcoded group counts, types too small to hold the aggregate, and unhandled overflow. It was, of course, faster. A digest catches that class of thing without anyone having to suspect it.

Canonicalised is the load bearing word, because three systems asked the same question return the same answer in three different shapes and none of those shapes is wrong. So the rendering is defined once, in the trait crate, and no driver renders its own results. Nulls have one spelling, text is escaped so that a value which happens to look like a null is not one, and drivers do not hash their own output, since a driver that hashes its own output can agree with itself about a wrong answer.

Two of the rules cost something and are worth stating. Floats are rendered to six significant digits, because two engines summing a hundred million values will not agree past that when one adds them in parallel partial sums and the other in order. Six digits survives that and still catches a hardcoded group count or an overflowed accumulator, neither of which is a seventh digit difference. What it gives up is a genuine small error, and that is the price of a digest, which cannot express a tolerance. Rows are sorted unless the query specified an order, because two systems that returned the same rows in a different order both answered the question that was asked, and a query that does specify an order gets that order checked.

## Pre-registration

Every claim this repository intends to test is registered before it is run, with the threshold that decides it. Rows carry a purpose field, either confirmatory or exploratory, and an exploratory row cannot be cited as a claim. A claim that was registered and not run shows in the ledger as pending, which makes quietly dropping an inconvenient result visible as a gap in a committed file.

## What gets thrown away

Only runs that failed a gate that was declared in advance. Removing an outlier after looking at it is prohibited, and so is running an experiment repeatedly and publishing the run that worked. The purpose field makes the second one mechanical rather than a matter of discipline.

## The full specification

This page is the summary. The complete method, including the result row schema, the storage tier definitions, the metric set, the instrumentation budget and the anti-pattern checklist, is published verbatim alongside the results.
