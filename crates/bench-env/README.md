# bench-env

Environment capture and machine eligibility gates.

Before a run takes a single measurement it asks this crate whether the machine is fit to produce the kind of number the run wants. The answer is yes, or it is a refusal naming the gate that failed. There is no third answer and there is no override, because a number with a warning attached is a number that gets copied without the warning and then it is on a slide somewhere.

A capture has two halves. The facts are what the machine is: the processor, the memory fitted, the frequency governor, the boost state, whether the run was pinned, whether there is a hypervisor underneath. They are stable until somebody reconfigures the machine, and they are what gets hashed into every result row. The conditions are what the machine is doing: free memory, load average, how many other processes are busy. They move between one measurement and the next, so they are gated and they are not hashed.

The rule that does most of the work is not a gate. A gate can only fail on something it can read, and the settings that matter most are the ones some platforms do not expose at all, so a machine whose clock settings cannot be read produces ratios rather than durations. Being unable to see a setting is not evidence that the setting is right.

Part of [iris-bench](https://github.com/tamnd/iris-bench). Licensed under Apache-2.0.
