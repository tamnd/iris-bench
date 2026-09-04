# The claim ledger

Every claim this project intends to make is registered here before it is run, together with the threshold that decides it. A claim that was registered and never run stays on this page as pending, which is the point: dropping an inconvenient result has to show up as a gap in a committed file rather than as a number that quietly never appeared.

A claim gets an identifier when it is registered, and the identifier is what other documents cite. `iris` cites them from its design notes, so a reader who wants to know where a number came from follows the identifier here and finds the threshold, the machine, the run and the raw data rather than a sentence asserting the number.

## How to read an entry

**Purpose** is either confirmatory or exploratory, and it is fixed at registration. An exploratory row cannot be cited as a claim anywhere. That is what stops a measurement taken to see what happens from being promoted to evidence once it comes out well.

**Threshold** is what the claim is judged against, and it is written before the measurement exists. Where the threshold lives somewhere else, the entry says which commit carries it, so that the ordering is checkable rather than asserted.

**Verdict** is recorded whichever way it goes. A claim that fails its own threshold stays on this page with the failure written out.

## This file is maintained by hand, for now

It should be generated from the results store, and it is not, because the store is a stub. A ledger somebody edits drifts within weeks and it drifts in the flattering direction without anybody deciding to do that, which is why replacing this file with a generated one is issue #27 rather than a nice to have. Until then the entries are written out longhand and each one names the artifact its numbers came from, so the drift is at least checkable against something.

## Registered

### C0001, what iris-guard costs

**Status** measured. **Purpose** confirmatory. **Registered** 2026-09-04, in `tamnd/iris` issue #17 and in the decision points table of that repository's `docs/ROADMAP.md`, which arrived in commit `ec37026`, the first commit of the repository. **Measured** 2026-09-05.

**Threshold**, three bands, committed in advance. Under five percent: the guard stays on and there is nothing further to say. Between five and fifteen: the guard stays on and digest pinning becomes the documented path for a host that needs the difference. Over fifteen: the guard stays on and its cost is a design problem rather than a check to remove. There is no band in which the check comes off, which is what makes this a gate rather than a benchmark.

**What is divided by what.** The numerator is `iris_guard::check` on a batch. The denominator is assembling that same batch into Arrow arrays, which is the tightest denominator available and is therefore the one that shows the guard in its worst light. A real scan also decodes inside the sandbox and that is where the time in a workload goes, so every share here is an upper bound on the share of a scan by some margin. Arrow's own validation stays inside the denominator rather than being subtracted out, because removing it would mean building arrays unchecked.

**Machine class.** The rows below are hosted continuous integration runners, which are shared machines, so the durations are not comparable across rows and are not quotable as durations. What they are for is the share and whether it changes character across architectures. The publishable number comes from the i9-13900K described in `MACHINES.md`, from the `Fleet` workflow.

**Verdict.** Over fifteen percent, on the string shape, on every platform measured. By the rule that means the guard stays on and its cost is a design problem. The other shapes land under six percent everywhere.

**Result.** Worst share per platform, three columns of 8192 rows, 1000 samples, produced by `cargo run --release -p iris-runtime --features probe --example guard_cost`, artifacts `guard-cost-<platform>` on `tamnd/iris` run 33901962630.

| Platform | int64-plain | int64-nullable | utf8 | Worst |
| --- | --- | --- | --- | --- |
| linux-x86_64 | 0.41% | 5.87% | 23.10% | 23.10% |
| linux-aarch64 | 0.90% | 2.50% | 22.63% | 22.63% |
| macos-aarch64 | 5.61% | 6.56% | 17.38% | 17.38% |
| windows-x86_64 | 0.47% | 2.92% | 22.08% | 22.08% |

The four rows agree on the band and on which shape decides it, which is the thing a matrix is for. Two architectures, three operating systems, one answer.

**Architecture note.** Counting the nulls in a validity bitmap is a popcount per byte, and the durations say it vectorises on both arm64 rows and does not on either x86-64 row: 500 and 592 nanoseconds against 3100 and 3105 for the same work. That is the baseline instruction set the runners build for rather than anything about the check, and it is the reason `int64-nullable` sits at 5.87% on one row and 2.50% on another. It does not move the verdict, which the string shape decides on every platform.

**Side result, not separately registered.** Checking a dictionary of 256 values behind 8192 keys costs 28 to 86 times what checking the plain `i64` column it stands for costs, in the same runs. The claim was registered on the expectation that encoded arrays would be the cheap case, on the reasoning that less data is less to walk, and the measurement says the opposite. A plain column of fixed width values is one multiplication because no bit pattern of eight bytes is out of range, while every dictionary key is a position into something else and has to be looked at. The guard charges for indirection rather than for bytes. This is recorded here rather than dropped because it is the part of the claim that turned out to be wrong.

## Not yet registered

The M0 numbers in `tamnd/iris` are not on this page. They were taken before this ledger existed, and back-dating a registration is exactly the move the registration is supposed to prevent, so they will be re-run against a registered claim rather than written up from the runs that already happened. The milestone that owns that work is B1.
