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

### C0002, what the sliding window costs on a file that is already resident

**Status** measured, and the threshold is not decidable with the instrument that exists. **Purpose** confirmatory. **Registered** 2026-09-04, in `tamnd/iris` issue #26 and in the M4 checklist of that repository. **Measured** 2026-09-06.

**Threshold**, committed in advance: a scan of a resident local file through the windowed path stays within three percent of a scan of the same bytes handed over as one buffer. There is no band structure here. Either the abstraction is free on the easy case or hosts will keep a second code path for resident files, which is the outcome the whole source design exists to avoid.

**What is divided by what.** The numerator is a scan of a `FileSource`, the denominator the same scan of a `MemorySource` over bytes read from the same file, both through `RangeSource` and in ranges of the same size. Samples are taken in pairs, back to back, with the order alternating. The work inside the loop is summing the bytes as little endian words, which is the cheapest per byte thing a scan can plausibly do and therefore the denominator that shows the window in its worst light.

**Machine class.** The i9-13900K under Linux, pinned, which is the only machine in the fleet whose noise floor in `MACHINES.md` sits under three percent. Ratios only, taken inside one run, which is what this claim is.

**Verdict.** Not held at the span iris ships, and not decidable at any span, for two separate reasons that are worth keeping apart.

At the four mebibyte span that `iris_source::DEFAULT_SPAN` sets, a 256 mebibyte scan runs at 143.51% of the buffer path, interval 142.63% to 144.42%. At a span that holds the whole file it runs at 96.72%, interval 96.46% to 97.11%. That is a large effect and its cause is not the abstraction.

**Result.** 60 pairs, 256 mebibyte file, 256 kibibyte ranges, `iris-bench resident`.

| Span | Slides per scan | Windowed | Whole buffer | Ratio | Interval |
| --- | --- | --- | --- | --- | --- |
| 4 MiB, what iris ships | 64 | 17.555 ms | 12.233 ms | 143.51% | 142.63% to 144.42% |
| 256 MiB, no sliding | 0 | 11.590 ms | 11.982 ms | 96.72% | 96.46% to 97.11% |
| control, two buffers | not applicable | 11.399 ms | 12.510 ms | 91.12% | 90.65% to 91.84% |

**The sliding cost is minor page faults, and it does not depend on how often the window slides.** A sweep from an eight mebibyte span to a 128 mebibyte one, which is 32 slides per scan down to two, reports 144.22%, 143.93%, 143.23% and 143.87%. Sixteen times fewer slides costs the same. Counting faults says why: differencing the minor fault totals of a 10 pair run and a 30 pair run gives exactly 4096 extra faults per scan at any sliding span, and zero at a span that holds the file. A 256 mebibyte file is 65536 pages, and Linux maps sixteen pages around a fault on a file backed mapping, so 4096 is the whole file re-established once per scan. A scan restarts at offset zero, so whatever the span is, the part of the file the window is not currently over has to be mapped again before the scan ends. The number of slides changes how the work is divided up and not how much of it there is.

**The three percent threshold cannot be decided by this instrument.** `iris-bench resident --control` compares the buffer path against a second buffer read from the same file by the same call, so it measures the harness and nothing else. It reports 91.12%, interval 90.65% to 91.84%: two copies of the same thing come out nine percent apart, and the side allocated second is the faster one every time. The effect tracks the working set rather than the code, at 96.90% for an eight mebibyte file that fits in the level three cache and 86.32% for a 64 mebibyte one that does not, which points at where the two allocations landed in physical memory rather than at anything either side is doing. Until that is understood the harness cannot honestly resolve a three percent difference, and the bar was left at three percent rather than widened to a number the instrument happens to clear.

**What can be said.** At a span that holds the file the windowed path is closer to the buffer path than a second copy of the buffer is: 96.72% against a control of 91.12%, on the same machine within minutes. That is not the registered claim and it is not written here as if it were. It is the strongest statement the measurement supports, and it says the cost worth chasing is re-establishing the mapping rather than the abstraction over it.

## Not yet registered

The M0 numbers in `tamnd/iris` are not on this page. They were taken before this ledger existed, and back-dating a registration is exactly the move the registration is supposed to prevent, so they will be re-run against a registered claim rather than written up from the runs that already happened. The milestone that owns that work is B1.
