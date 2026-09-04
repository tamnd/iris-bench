# bench-store

Append only result storage and the claim ledger.

Raw iterations are stored, never summaries. Aggregation happens at read

time, because storing only medians would make it impossible to re-analyse

with a different statistic later and re-running is expensive.

Nothing is deleted. Corrections are new rows that reference the run they

correct.

Part of [iris-bench](https://github.com/tamnd/iris-bench). Licensed under Apache-2.0.
