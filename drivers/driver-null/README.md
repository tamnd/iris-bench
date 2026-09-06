# driver-null

A driver that does nothing.

Running the harness against a system that does no work measures what the harness itself costs. The instrumentation budget is one percent, and this is how it is checked.

It is not a system under test and it never appears in a results table. It implements the trait in full and returns an empty result from every query, so every digest it produces disagrees with every real system, which is the right answer for a driver that computed nothing.

Part of [iris-bench](https://github.com/tamnd/iris-bench). Licensed under Apache-2.0.
