# iris-bench-cli

`iris-bench`, the command line tool.

Everything this repository can do from a terminal is here and nowhere else, so the tool is the only way anybody runs anything and there is no second path with different defaults on it. `check` says what the machine is and which eligibility gates it passes. `noise`, `overhead` and `resident` are the measurements about the harness itself. `corpus` fetches or generates a pinned corpus into the store. `clickbench` runs the workload against one system, compares the records afterwards, and calibrates them against the public leaderboard.

`run`, `reproduce` and `report` are named and do nothing yet. They are the milestones after this one, and they are in the command list rather than absent from it so that the shape of the finished tool is visible from the help text.

The parts that would be tempting to default are command line arguments instead. Thread count, memory budget, the seed, whether the page cache was dropped, and whether a machine that failed its gates was measured anyway are all in the record, because a reader of a number should not have to know which version of this tool produced it to know what it means.

It refuses to produce a publishable number on a machine that failed its gates, and it says which gate failed. That is not configurable, only overridable in a way that marks the result.

Part of [iris-bench](https://github.com/tamnd/iris-bench). Licensed under Apache-2.0.
