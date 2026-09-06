# driver-datafusion

The DataFusion driver.

DataFusion is a query engine over files rather than a database with its own storage, so this driver registers the corpus where it lies instead of ingesting it, which is what DataFusion's own published benchmark entries do. Loading costs almost nothing and every scan is paid for in the run phase, which is a different trade from the one DuckDB makes and exactly the difference the three phases exist to show. `CONFIG.md` names every setting, where it came from, and what that choice costs a reader comparing the two.

Partition count, thread count and memory budget come from the machine class rather than from DataFusion's defaults, because those defaults read the host.

Part of [iris-bench](https://github.com/tamnd/iris-bench). Licensed under Apache-2.0.
