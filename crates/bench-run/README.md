# bench-run

The runner.

Owns process isolation, randomised ordering with a recorded seed, the five

storage tiers, and cache control. Warm up is the runner's job and not the

driver's, which is how one system does not get warmed while another does

not.

Part of [iris-bench](https://github.com/tamnd/iris-bench). Licensed under Apache-2.0.
