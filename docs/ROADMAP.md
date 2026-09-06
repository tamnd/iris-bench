# Roadmap

Nine milestones, each finished when its exit gate passes rather than when the code is written. Every gate is an issue in the matching [GitHub milestone](https://github.com/tamnd/iris-bench/milestones).

The ordering is driven by one constraint: this harness has to be able to measure things before `iris` has anything worth measuring. Everything up to B4 is useful with zero `iris` code in existence, which is what makes the two projects independently viable and what makes the comparisons credible when `iris` finally does show up.

## B0, can we measure at all

No benchmarks. The measurement floor, established first, because every threshold worth testing is smaller than the noise on an unprepared machine.

The work: eligibility gates that abort on failure and emit an environment capture, a noise floor probe on each machine role, an instrumentation overhead measurement against the null driver, and the timing loop with a bootstrap confidence interval.

**Gate.** The noise floor on the primary x86-64 role is measured and under 1%, and the null driver shows under 1% instrumentation overhead. If the floor is above 2%, the machine is not fit for purpose and the 5% decision rules that `iris` turns on cannot be settled on it. Finding that out in week one is the point.

### The B0 decision

Both halves are measured, they answered differently, and the decision that comes out of them is written here rather than left implied. The numbers and how they were taken are in `MACHINES.md`, this is what follows from them.

**One class holds under 2% and it is the only one, so absolute durations are a narrow permission rather than the default.** The round to round floor is 1.05% on the i9-13900K under Linux, pinned to the performance cores, with every gate in its set passing. It is the only row in the fleet taken on a machine that satisfied its own gate set and the only one under the bar for a reason about the machine rather than about the moment. Everything else is over: the three virtualised EPYC guests between 3.89% and 39.68%, the same workstation under Windows at 18.54%, macOS at 7.83%. The hosted arm64 runner reads 0.10% and 0.23% within a job, and that is not a floor, because what makes a hosted runner ratios only is the spread between jobs and three jobs is not a sample of that.

So the rule is one class, one operating system, one configuration, and it is checked at run time rather than remembered. `iris-bench check` evaluates the gate set before a run and aborts by name when it fails, so a duration cannot be produced on a machine that was not entitled to produce it. Every other machine in the fleet publishes ratios taken inside one run, where it is its own control. Where an absolute number does get published from the one eligible role, the machine and the floor go in the same sentence as the number and not in a footnote, because a caveat in a footnote is a caveat that gets dropped when the number is quoted.

**Instrumentation overhead is under 1%, but not as a single number, and the timing approach does not change.** The harness adds 41.1 ns to a reported sample on the eligible role, so it is under one percent of any workload longer than 4.1 us and it is 0.57% at ten microseconds. Two repeat runs gave 37.2 and 37.1 ns. There is no one percentage to quote because the cost is roughly fixed and the share it takes depends entirely on how long the workload runs, and the gate as written above asked for a figure in a form that does not exist. The form that does exist is the fixed cost plus the duration at which it crosses the bar, and it is the more useful of the two, because it applies to a workload nobody has written yet.

What that buys is a limit on which workloads can be timed at all rather than a change to how they are timed. A workload under about ten microseconds does not get published as a duration from this harness without `iris-bench overhead` being re-run at that duration first. Every workload B1 through B8 contemplates is milliseconds or longer, so nothing downstream is blocked, and the reason this is written down anyway is that the constraint binds on microbenchmarks, which are exactly the thing somebody reaches for when a large result needs explaining.

**What is still not settled.** The `busy-processes` gate wants nothing else above five percent of a processor and no machine in this fleet meets it reliably, including the eligible one, which reported one busy process at its quietest and thirty three a few minutes later with nothing started in between. That gate was deliberately not loosened to let these measurements through. The fleet has no bare metal Linux machine and no AVX-512 anywhere, and neither of those is fixable by trying harder. So B0 answers can we measure at all with yes, on one machine, for ratios always and for durations under a gate, and it does not answer can we measure everything.

## B1, corpora and provenance

Manifests, fetching, generation, verification and the content addressed store.

**Gate.** ClickBench `hits`, TPC-H at scale factors 1 and 20, Public BI in both the 36 dataset subset and the full set, Silesia and enwik8 all fetch or generate to matching digests. The assertions in the manifests pass, so ClickBench is 99,997,497 rows across 105 columns or the manifest is wrong. Generated corpora produce identical digests on Linux x86-64, Linux arm64 and macOS, or the divergence is documented and the corpus is redefined as platform pinned. Every mirrored corpus has a licence note and CI enforces it. A digest mismatch on fetch is a hard failure that prints both digests.

### The B1 outcome

Every gate above is met, and three of them turned out to be worth more than they read like on paper. Measured facts are in `CORPORA.md`, this is what follows from them.

**Every corpus was fetched or generated end to end rather than checked on paper.** That is 43,334,957,146 bytes of Public BI across 206 files, 22.5 GB of TPC-H at scale factor 20, 13.8 GiB of ClickBench, and Silesia and enwik8 from the mirror. The reason to say it out loud is that a manifest whose digests were transcribed from somewhere else is a manifest that has never been tested, and the failure mode of one is that it looks exactly like a manifest that has. Public BI paid this back immediately: 62 of its 206 files were already in the store when the full set was fetched, 36 from the subset and 26 as duplicates of tables arriving earlier in the same run, which is the content addressing doing what it was built for rather than a case anybody handled.

**Three platforms produce identical digests and the fourth cannot, for a reason in the tool.** TPC-H scale factor 1 comes out byte for byte the same on macOS on Apple silicon, Linux on x86-64 and Linux on arm64. On Windows it cannot, because `dbgen` opens its output with `fopen(path, "w")` and the C runtime there rewrites every newline. So the corpus is platform pinned, as the gate allows, and `[generator] platforms` is the pin: a required list of the operating systems the bytes have actually been produced on, checked before the generator starts. It blocks the comparison at its source rather than at the report, because a machine that cannot produce the corpus cannot measure on it.

**No override means a check that reads the source, not a rule somebody remembers.** A digest mismatch was already a hard failure and nothing skipped it, but that was true because the code happened to be written that way. `check_digest_is_not_overridable` now fails the build on an override name, a read of the environment, or an insert that does not name the pinned digest, anywhere in the four files a corpus's bytes pass through. The failure path is tested through a real HTTP server serving wrong bytes at the right length, and through the whole `iris-bench corpus` command against a generator that writes the wrong thing.

**What is still not settled.** Scale factor 20 has not been generated on arm64 Linux, because no machine in this fleet has 22 GiB free on that architecture, so its cross-platform evidence is two platforms rather than three and the third is inferred from scale factor 1 rather than measured. enwik9 is not mirrored, because nothing measures it yet. And the mirror is a release on this repository, which makes a mirror that has stopped working and a repository that has stopped existing the same event, which is a property worth having and not the same thing as durability.

## B2, drivers and the first real comparison

The runner, the store, and drivers for DuckDB, DataFusion and the arrow-rs Parquet reader. Still no `iris`.

**Gate.** The 43 ClickBench queries run under ClickBench's own rules and the geometric mean lands within 25% of the public leaderboard for the same system and machine class. This is the harness validating itself against a public number, and it is the most valuable gate in the plan. Result digests match across all three systems for all 43 queries, or the mismatches are investigated and explained before anything else proceeds. Randomised ordering is on by default, the seed is recorded, and a re-run with the same seed reproduces the schedule. Every driver has a `CONFIG.md` that passes the CI checks.

## B3, reproduction machinery

The `reproduce` command, and then the highest value experiment in the repository.

**Gate.** The F3 artifact runs per its own reproduction documentation on Linux x86-64 and emits a verdict. The same runs on Linux arm64, which is an artifact documented as tested only on Intel hardware running Debian, run on an architecture its authors did not test. ALP's compression table reproduces within two percent, which is the calibration target: compression ratios are deterministic, so a failure there means the harness is wrong and B2's gate passed by luck. Artifacts that cannot be obtained are recorded as unavailable with a citation rather than omitted. Verdicts appear in the claim ledger with run identifiers.

This milestone produces a publishable result with no `iris` code written. That is deliberate. If `iris` is never built, B3 still contributed something.

## B4, the held-out corpus

Timed on purpose: before any `iris` decoder is developed, so that the ordering requirement is satisfied by the schedule rather than by good intentions. A held-out corpus assembled after tuning is not held out, and that cannot be recovered later.

**Gate.** Selection criteria published and committed before any format is run against it. The corpus is content addressed and frozen, and the manifest digest is in git history with a date. Every format in the matrix is measured on both Public BI and the held-out corpus, and the delta is reported per format. The interpretation is applied as written, including the clause saying that `iris` degrading least is not evidence that `iris` is better.

## B5, storage tiers and the latency sensitivity plot

Tier support in the runner, MinIO with injected latency, real object storage, and reconciled request accounting.

**Gate.** Client side and server side byte and request counts reconcile within one percent at the network tier, or the run is quarantined. The latency sweep across 0.1, 1, 10, 40 and 100 milliseconds of injected round trip time produces a slope per format. The table showing which formats change rank between a warm local buffer and real object storage exists. Cold cache probes confirm the drop actually happened, per platform.

The latency sensitivity plot is the second publishable artifact that does not require `iris` to exist, and it is the clearest test of the thesis whichever way it comes out.

## B6, iris enters the matrix

A driver consuming a released `iris` artifact, not a working tree.

**Gate.** The `require_range` cost claim and the windowing overhead claim are measured and recorded here rather than in the `iris` repository, which is what makes them credible. `iris` appears in results with no visual distinction and no privileged configuration path. ClickBench runs in both shapes, partitioned across four files for comparability with the published AnyBlox numbers and as a single file which is what `iris` claims to support. The self-decoding overhead numbers are published, including the crossover file size below which shipping the decoder costs more than the encoding saves.

## B7, the arm64 study

The research contribution.

**Gate.** WebAssembly against native decode ratios measured on Linux x86-64 and Linux arm64 for the same decoders, with instruction counts and instructions per cycle, so the mechanism is identified and not merely the gap. The worst case that the literature reports for string decoding published on both architectures and with the native fast path, in one table, x86-64 first because that is where we are weakest. The verdict is recorded whichever way it goes, and context dependent is the expected and most interesting outcome. Adversarial review completed, with the counter-argument published alongside.

Read `MACHINES.md` before reading any result from this milestone. There is no AVX-512 in the fleet, which makes the measured x86 gap a lower bound.

## B8, continuous operation and publication

Change point detection, the site, the claim ledger, and the export into the `iris` design notes.

**Gate.** Change point detection running nightly on the dedicated roles with persistent stateful triage, so a point marked a false positive stays muted rather than reappearing tomorrow. The site renders from the store with all seven rendering rules enforced in code. The export fragment is generated, and `iris` CI checks that every unverified marker in its documents either stays a marker or cites a live claim identifier. One full release bundle reproduces its own results on the same machine within the noise budget. The self-audit is answered in writing and committed.

## Decision points

| After | Question | If the answer is bad |
|---|---|---|
| B0 | Is the noise floor under 1%? | Above 2% and the 5% thresholds cannot be settled here. Get better hardware before building anything. Answered above: one class is under, on one operating system, and it is 1.05% |
| B2 | Do we land within 25% of the public ClickBench leaderboard? | The harness has a systematic error and nothing downstream is trustworthy |
| B3 | Does the ALP table reproduce within two percent? | Compression ratios are deterministic, so a failure means a corpus or reader bug |
| B3 | Does F3 hold on arm64? | Either outcome is publishable, and if F3 wins then `iris` has design lessons to take |
| B4 | Do all formats degrade similarly on held-out data? | Similar is a finding about the field. Different is a finding about overfitting |
| B5 | Does the ranking reverse between tiers? | A stable ranking means the field's warm local evaluations were fine, and we should say so plainly |
| B6 | Does `iris` beat a well configured Parquet reader on bytes transferred? | If not, the range inversion buys nothing over the status quo |
| B7 | Is the WebAssembly gap smaller on arm64? | If not, the vector width diagnosis is wrong and the native fast path becomes mandatory everywhere |

## Risks

| Risk | Severity | Mitigation |
|---|---|---|
| Noise floor too high to settle five percent effects | High | B0 finds out in week one, before anything is built on it |
| Corpus rot, since some of these tarballs are over a decade old | High | Digests recorded even where mirroring is not permitted |
| A rival's maintainers say we configured them wrong | Medium | That is the expected outcome of the review protocol, which is why the window exists before publication |
| Object storage cost and variance | Medium | MinIO carries the mechanism and real storage confirms it. Bytes and requests over wall clock |
| A competing system simply wins | Medium | Not a bad outcome. We learn either way, and that is written down in advance |
| Scope creep into a general benchmarking framework | Medium | Stated non-goal |
| One maintainer, two repositories | High | B0 through B4 need no `iris` code, so the two tracks do not block each other |

## What to do first

B0, one week. Then B2's leaderboard gate, because landing within 25% of a public ClickBench number is the cheapest possible proof that the harness is not systematically wrong, and everything else depends on it. Then B3's arm64 reproduction, which is a competitor's public artifact run on hardware its authors did not test, and which requires no `iris` code. Then B4's corpus freeze, before any decoder is tuned, because that one is ordering sensitive and cannot be recovered later.
