# bench-core

Timing, repetition and the result row.

No benchmark lives in this crate. It holds the measurement loop, the

bootstrap confidence interval on the median, and the row schema that every

result is written as. Keeping benchmarks out of it is what lets the timing

code be reviewed in one sitting.

Part of [iris-bench](https://github.com/tamnd/iris-bench). Licensed under Apache-2.0.
