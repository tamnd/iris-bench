# Machines

Results are labelled by hardware class, never by machine name, because a hostname tells a reader nothing and a CPU model tells them everything. This page lists what is actually available, what each class is used for, and what it must not be used for.

Two things follow from this list rather than from any design decision, and they change what this repository can claim. Both are stated here rather than buried in a results footnote.

## The fleet

### Class A, virtualised AMD EPYC servers

Three virtual machines on KVM and QEMU, all running Ubuntu 24.04 LTS on the 6.8 kernel series, all reporting an AMD EPYC processor as exposed by the hypervisor.

| | vCPUs | Memory | Storage |
|---|---|---|---|
| A1 | 4 | 5 GiB | virtual disk |
| A2 | 6 | 11 GiB | 200 GB virtual disk |
| A3 | 8 | 23 GiB | 400 GB virtual disk |

Instruction set as exposed to the guest tops out at AVX2. There is no AVX-512.

These are shared tenancy virtual machines. The guest cannot set the CPU frequency governor, cannot disable turbo, cannot pin to a physical core with any confidence that it is exclusive, and cannot see what a neighbouring tenant is doing. That rules them out for any published timing. The noise floor section below puts a measured number on it: between 3.89% and 39.68% round to round on an identical workload, against a two percent bar.

**Used for:** hosting the object storage endpoints for the storage tier study, including MinIO with injected network latency; mirroring corpora; running generation jobs, digest verification and other work whose output is a file rather than a duration; long running jobs where wall clock is not the measurement.

**Not used for:** any number that appears in a result row with a duration in it. The eligibility gates reject them, and that is correct.

### Class B, Intel Core i9-13900K workstation

One machine. Raptor Lake, 8 performance cores and 16 efficiency cores for 32 threads, 64 GB of DDR5, a Kingston KC3000 1 TB NVMe drive on PCIe 4.0, and an RTX 4090 that nothing here uses. It runs Windows 11 Pro, with WSL2 hosting Ubuntu 26.04 LTS on the 6.18 kernel series and 31 GiB of the 64 GB visible to the Linux side.

Instruction set: AVX2 and AVX-VNNI. Raptor Lake has AVX-512 fused off, so there is none here either.

This is the primary timing machine, and it comes with three caveats that go on every number it produces.

It is a hybrid part. A run that is allowed to migrate between a performance core and an efficiency core produces a distribution with two modes and a confidence interval that means nothing. Runs pin to performance cores, and the pinning is recorded in the environment capture rather than assumed.

The Linux side runs under WSL2, which is a hypervisor layer with its own memory ballooning and its own filesystem behaviour. That is fine for within-run ratios, which is what most of the interesting quantities here are, and it is a known unknown for absolute numbers. Replacing it with a bare metal Linux install on the same hardware is the single cheapest improvement available to this repository, and it is tracked as an issue.

Windows numbers come from the same machine natively, which makes the Windows against Linux comparison a comparison of operating systems on identical hardware, which is unusually clean.

**Used for:** the x86-64 Linux timing role, the Windows timing role, and the noise floor probe that decides whether either is fit for purpose on a given day.

### Class C, hosted arm64 Linux

GitHub Actions `ubuntu-24.04-arm`. There is no arm64 Linux machine in the fleet, so this is the only arm64 Linux available.

A shared hosted runner has a job to job spread in the range of five to fifteen percent, which is larger than most of the effects worth detecting, because a job lands on whatever physical host is free and next to whatever neighbours are on it. So arm64 results are within-run ratios only: WebAssembly against native on the same machine at the same moment, or guarded against unguarded. Absolute arm64 timings are not published, and a cross machine comparison against a Class B absolute number is prohibited rather than discouraged.

Inside a single job the runner is quiet, and measurably so: the noise floor section below records 0.10% and 0.23% round to round within a job. That is the reason a within-run ratio taken there is worth having, and it is not a reason to relax the paragraph above, because the two numbers are about different things.

This is a real limitation and it is worth being blunt about it, because the arm64 study is the part of this work most likely to produce a new result. A ratio measured on a noisy machine is still a ratio, but it is a weaker instrument than the same ratio on dedicated hardware.

### Class D, macOS on Apple silicon

The development machine. Used for portability checking and for making sure the harness builds and runs, and for nothing that gets published.

## The gate sets

Every class above has a gate set, and `iris-bench check` evaluates it before a run takes a measurement. A gate that fails aborts and names itself. There is no warning and no override, because a number with a caveat attached is a number that gets copied without the caveat.

Three gates apply everywhere they can be evaluated. `memory-headroom` wants a quarter of the fitted memory still available, and never less than 2 GiB, because a machine with less than that free has something substantial resident in it and whatever that is has a working set competing for the same cache. `busy-processes` wants nothing else above five percent of a processor. `load-average` wants no more than a fifth of a load unit per logical processor, and exists only where the platform keeps a load average, which is everywhere except Windows.

Class B carries three more. `frequency-governor` wants the performance governor on Linux, and its Windows counterpart `power-scheme` wants the High performance or Ultimate Performance scheme. `turbo` wants boost off, so the clock does not fall away as the part warms up. `core-pinning` wants the run restricted to a subset of the processors rather than free to move across all of them, because a run that can migrate between a performance core and an efficiency core produces a distribution with two modes and a confidence interval over two modes describes neither of them.

### What a class can produce at best

| Class | At best | Measured floor | Why it is capped there |
| --- | --- | --- | --- |
| A, virtualised EPYC | nothing published | 3.89% to 39.68% | the guest cannot set the governor, cannot disable boost, cannot pin to a physical core and cannot see what a neighbour is doing |
| B, the i9-13900K under Linux | durations, or ratios | 1.05% | durations when the governor, the boost state and the affinity can all be read, ratios when they cannot, and ratios under WSL2 whatever else is readable |
| B, the i9-13900K under Windows | ratios only | 18.54% | the boost state and the processor affinity cannot be read there by an unprivileged process, and the measured floor is over the two percent bar |
| C, hosted arm64 | ratios only | 0.23% inside a job, not measured between jobs | a shared hosted runner has a run to run spread of five to fifteen percent |
| D, macOS | nothing published | 7.83% | the development machine, which exists to check that the harness builds and runs |

The measured floor column is the round to round spread from `iris-bench noise`, and the section below says how it was taken and what it does not cover.

The class B rows are the ones worth reading twice, because they are the rule that does most of the work and it is not a gate. A gate can only fail on something it can read, and the settings that matter most are exactly the ones some platforms do not expose. Under Windows the boost state and the processor affinity cannot be read by an unprivileged process, so nothing has checked whether the clock is steady, so that machine produces ratios. Being unable to see a setting is not evidence that the setting is right.

That is not a hypothetical. The M0 probe in iris ran on this machine under Windows and produced a windowed overhead anywhere between minus eight and plus eighteen percent across twenty four runs in one sitting, on a gate set at three, with the flat scan alone moving by a third across an hour. The ceiling rule predicted that before the measurement was taken, which is the argument for keeping it.

A setting that cannot be read is recorded as unreadable rather than skipped, and that recording goes into the environment hash. So the same machine measured under two operating systems produces two different hashes and a row from one cannot be quietly compared against a row from the other.

## The noise floor

Every effect this repository reports sits on top of how far a machine's answer moves when the question did not change. A difference smaller than that is not a difference, it is the machine. `iris-bench noise` measures it, and the table below is what the fleet actually did rather than what it was expected to do.

The probe walks four mebibytes of memory in four independent dependency chains. No I/O, no allocation inside the timed region, and a working set larger than any level of cache on any machine here, so what it exercises is the memory path and the scheduler rather than a library. Each round is a whole fresh process, because restarting re-rolls address space layout, page placement and allocator state, and a floor measured inside one long lived process comes out well below the floor a real comparison has to stand on. Every reading below is forty rounds of a hundred samples with five warmup passes per round.

Two numbers come out of it and they answer different questions. Round to round is the spread of the round medians, and that is the floor, because it is what a second run of a benchmark has to clear. Within one round is the spread of the individual samples inside a round, and it is a diagnostic rather than a limit, because a median over a hundred samples absorbs a descheduling event that a single sample does not. A machine can be steady between rounds and wild inside them, or the reverse, and both of those happened here.

| What was measured | Round to round | Within one round | Median round | Gates at the time |
| --- | --- | --- | --- | --- |
| A1, the 4 vCPU guest | 5.09% and 6.16% | 39% and 114% | 0.156 ms and 0.169 ms | busy processes and load average both failing |
| A2, the 6 vCPU guest | 9.98% and 39.68% | 160% and 149% | 0.201 ms and 0.223 ms | busy processes and load average both failing |
| A3, the 8 vCPU guest | 3.89% and 6.45% | 36% and 41% | 0.198 ms and 0.202 ms | busy processes failing |
| B, Linux under WSL2, pinned to the performance cores | 1.05% | 11.87% | 0.090 ms | every gate passing |
| B, Windows, pinned to the same sixteen processors | 18.54% | 4.22% | 0.096 ms | busy processes failing, at one process |
| C, hosted arm64 | 0.10% and 0.23% | 3.0% and 2.4% | 0.155 ms | busy processes failing, the runner agent |
| D, macOS | 7.83% | 26.80% | 0.261 ms | busy processes and load average both failing |

Where there are two figures they are two separate sittings, and both are printed because the difference between them is part of the result.

The bar is two percent. Class A is over it on every machine and in every reading, which is the answer this measurement existed to get. The three guests were already ruled out for published durations, but that was an argument from what the hardware exposes to a guest. Now there is a number attached to it, and the number is worse than the argument suggested. The 6 vCPU guest moved by 39.68% between rounds of an identical workload in one sitting, with its slowest round more than three times its fastest. Any effect smaller than that measured on that machine is the neighbouring tenants.

Class A is also not stable between sittings, and that matters more than any single figure in the row. The 8 vCPU guest gave 3.89% at a load average of 6.05 and then 6.45% an hour later at a load average of 1.42, so the quieter reading was the worse one. A floor that moves in the wrong direction when the machine calms down is not a property of the machine, and a class whose floor cannot be pinned down is a class that cannot carry a duration.

Class B under WSL2, pinned to the performance cores with every gate in its set passing, came in at 1.05%. It is the only row here taken on a machine that satisfied its own gate set, and it is the only row under the bar for a reason about the machine rather than about the moment. On this evidence the durations ceiling for the Linux side stays where it is.

Class B under Windows on the same physical machine, pinned to the same sixteen processors, came in at 18.54%, and the shape of that number is unlike anything else in the table. The samples inside a round are the steadiest of any machine here at 4.22%, and the median round is 0.096 ms, within seven percent of the Linux side on the same silicon. What moves is whole rounds: the slowest is 0.163 ms against a fastest of 0.095 ms. So the Windows floor is not a slow machine, it is a machine that now and then hands a whole process to something else for a while. The ceiling rule had already put Windows at ratios because the boost state and the affinity cannot be read there. The measurement agrees with the rule, and the rule got there first, which is the argument for keeping rules that do not depend on a measurement being taken.

Class C measured 0.10% and 0.23% in two jobs, and three jobs in a row produced a median round of 0.155 ms. That does not lift the arm64 ceiling and it should not be read as evidence for lifting it. What the probe sees is one job on one rented machine over about ten seconds. The reason a hosted runner is ratios only is the spread between jobs, which is a question about which physical host a job lands on and how loaded its neighbours are, and three jobs is not a sample of that. The useful reading of this row is narrower and still worth having: the runner is quiet while it is running, so a ratio taken inside one job there is measuring what it claims to measure.

Class D sits at 7.83%, taken on the development machine at a load average of 30.90 while it was doing everything a development machine does. That class publishes nothing, so the row is a check that the probe reports something sane on a busy machine rather than a limit on anything.

### What the floor does not cover

Three gaps, written down so the table does not get read as more than it is.

It covers one sitting and one build. Drift across an hour and drift across a rebuild are both real, both larger than anything here on some machines, and both out of scope for this probe. The M0 work in iris watched a flat scan move by a third across an hour on the class B machine, which is more than fifteen times the round to round floor measured on it.

Every row except the pinned WSL2 one was taken with a gate failing, through `--anyway`, and the report prints that fact on its own output. Those rows describe a machine on a day and not the machine. That is not carelessness about the conditions, it is what the fleet is: the class A guests host the object storage endpoints and the generation jobs this page says they are for, so they are never idle by design, and a hosted runner has the runner agent on a processor for the whole job.

`busy-processes` wants nothing else above five percent of a processor and no machine here meets it reliably. The workstation reported one busy process at its quietest and thirty three a few minutes later without anything being started in between. That gate is tracked as its own issue with these readings as the evidence. It was deliberately not loosened to let these measurements through, because widening a bar until your own number gets past it is the same mistake as adding repetitions until a comparison turns significant.

## The AVX-512 problem

There is no AVX-512 anywhere in this fleet. The EPYC virtual machines expose AVX2 as their ceiling, and Raptor Lake has the unit fused off. This is not a configuration choice and it cannot be worked around by trying harder.

Three consequences, all of which change what gets claimed:

**AnyBlox's AVX-512 comparisons cannot be reproduced here.** Their evaluation ran on a Xeon with AVX-512 available, and the parts of Figure 9 that turn on 512 bit vector width are not attemptable on this hardware. The reproduction target that covers them is recorded as `NOT-ATTEMPTABLE` with this page as the reason, which is a statement about our machines and not about their work.

**The WebAssembly against native gap measured here is a lower bound on the x86 gap.** WebAssembly's vector width is capped at 128 bits and will be for the foreseeable future. Against AVX2 that is a four times width disadvantage on 32 bit lanes. Against AVX-512 it would be eight times. So a gap measured on this fleet understates what the same decoder would show on a machine with AVX-512, and every claim about that gap says so.

**The arm64 hypothesis gets easier to test and harder to interpret.** Arm Neon is 128 bits wide, the same as WebAssembly, which is the whole reason to expect the sandbox penalty to be smaller on arm64. Comparing a 128 bit guest against a 256 bit AVX2 host is a cleaner comparison than against a 512 bit host, but it also means a result showing a small gap on x86 might be a fact about AVX2 rather than a fact about the sandbox. The instruction count and instructions per cycle diagnostics exist so the mechanism is identified and not merely the gap, and on this fleet they are not optional.

## What the fleet is missing

Stated plainly, so that nobody has to reverse engineer it from the results.

- No dedicated bare metal Linux machine. The primary Linux role runs under WSL2.
- No arm64 Linux machine. Arm64 work depends on a shared hosted runner and is limited to ratios.
- No AVX-512 host of any kind.
- No machine with more than 64 GB of memory, which caps the scale factors that can be run without spilling and makes TPC-H beyond SF100 impractical.
- No dedicated network between the compute machine and the object storage endpoints, so the storage tier study uses injected latency on a controlled link rather than a real wide area path, with real object storage used to confirm the mechanism rather than to establish it.

Every one of these is a real constraint on what this repository can claim, and each of them is cited from the results that it affects.
