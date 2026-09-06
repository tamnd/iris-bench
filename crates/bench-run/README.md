# bench-run

The runner.

Owns process isolation, randomised ordering with a recorded seed, the five storage tiers, and cache control. Warm up is the runner's job and not the driver's, which is how one system does not get warmed while another does not.

Cache control is the part that exists so far. ClickBench reports a cold number and a hot number, and the cold one means nothing unless the page cache was really empty when the query started. Dropping it needs root on Linux and there is no mechanism here for anything else, so a run that could not drop it records why and the label travels with the number rather than being lost between the machine and the table.

Part of [iris-bench](https://github.com/tamnd/iris-bench). Licensed under Apache-2.0.
