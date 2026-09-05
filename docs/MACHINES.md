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

These are shared tenancy virtual machines. The guest cannot set the CPU frequency governor, cannot disable turbo, cannot pin to a physical core with any confidence that it is exclusive, and cannot see what a neighbouring tenant is doing. That rules them out for any published timing.

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

A shared hosted runner has a run to run spread in the range of five to fifteen percent, which is larger than most of the effects worth detecting. So arm64 results are within-run ratios only: WebAssembly against native on the same machine at the same moment, or guarded against unguarded. Absolute arm64 timings are not published, and a cross machine comparison against a Class B absolute number is prohibited rather than discouraged.

This is a real limitation and it is worth being blunt about it, because the arm64 study is the part of this work most likely to produce a new result. A ratio measured on a noisy machine is still a ratio, but it is a weaker instrument than the same ratio on dedicated hardware.

### Class D, macOS on Apple silicon

The development machine. Used for portability checking and for making sure the harness builds and runs, and for nothing that gets published.

## The gate sets

Every class above has a gate set, and `iris-bench check` evaluates it before a run takes a measurement. A gate that fails aborts and names itself. There is no warning and no override, because a number with a caveat attached is a number that gets copied without the caveat.

Three gates apply everywhere they can be evaluated. `memory-headroom` wants a quarter of the fitted memory still available, and never less than 2 GiB, because a machine with less than that free has something substantial resident in it and whatever that is has a working set competing for the same cache. `busy-processes` wants nothing else above five percent of a processor. `load-average` wants no more than a fifth of a load unit per logical processor, and exists only where the platform keeps a load average, which is everywhere except Windows.

Class B carries three more. `frequency-governor` wants the performance governor on Linux, and its Windows counterpart `power-scheme` wants the High performance or Ultimate Performance scheme. `turbo` wants boost off, so the clock does not fall away as the part warms up. `core-pinning` wants the run restricted to a subset of the processors rather than free to move across all of them, because a run that can migrate between a performance core and an efficiency core produces a distribution with two modes and a confidence interval over two modes describes neither of them.

### What a class can produce at best

| Class | At best | Why it is capped there |
| --- | --- | --- |
| A, virtualised EPYC | nothing published | the guest cannot set the governor, cannot disable boost, cannot pin to a physical core and cannot see what a neighbour is doing |
| B, the i9-13900K | durations, or ratios | durations when the governor, the boost state and the affinity can all be read, ratios when they cannot, and ratios under WSL2 whatever else is readable |
| C, hosted arm64 | ratios | a shared hosted runner has a run to run spread of five to fifteen percent |
| D, macOS | nothing published | the development machine, which exists to check that the harness builds and runs |

The class B row is the one worth reading twice, because it is the rule that does most of the work and it is not a gate. A gate can only fail on something it can read, and the settings that matter most are exactly the ones some platforms do not expose. Under Windows the boost state and the processor affinity cannot be read by an unprivileged process, so nothing has checked whether the clock is steady, so that machine produces ratios. Being unable to see a setting is not evidence that the setting is right.

That is not a hypothetical. The M0 probe in iris ran on this machine under Windows and produced a windowed overhead anywhere between minus eight and plus eighteen percent across twenty four runs in one sitting, on a gate set at three, with the flat scan alone moving by a third across an hour. The ceiling rule predicted that before the measurement was taken, which is the argument for keeping it.

A setting that cannot be read is recorded as unreadable rather than skipped, and that recording goes into the environment hash. So the same machine measured under two operating systems produces two different hashes and a row from one cannot be quietly compared against a row from the other.

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
