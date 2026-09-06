# bench-run

The runner.

Owns process isolation, randomised ordering with a recorded seed, the five storage tiers, and cache control. Warm up is the runner's job and not the driver's, which is how one system does not get warmed while another does not.

Cache control is one of the two parts that exist so far. ClickBench reports a cold number and a hot number, and the cold one means nothing unless the page cache was really empty when the query started. Dropping it needs root on Linux and there is no mechanism here for anything else, so a run that could not drop it records why and the label travels with the number rather than being lost between the machine and the table.

The other is query ordering. Running the queries in the order the file lists them lets each one start on a machine the query before it left behind, and which queries benefit from that is a property of the file rather than of anything being measured. The order comes from a seed instead, the seed is written down next to the numbers, and handing the same seed back gives the same order. What that removes is warming between queries. It does not remove warming inside a query, and it must not, because each query is run three times back to back and the second and third runs being warmed by the first is the definition of the hot number.

Part of [iris-bench](https://github.com/tamnd/iris-bench). Licensed under Apache-2.0.
