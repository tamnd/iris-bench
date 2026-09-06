# bench-workload

The published workloads, carried verbatim, and the rules their authors run them under.

A result is only comparable with somebody else's if it asked the same questions the same way. This crate holds both halves of that. The query text is carried byte for byte from where its authors publish it, with the address it came from and the digest it hashed to on the day it was fetched, and a test fails the build if the text ever drifts from that digest. The protocol is the publishers' protocol rather than one invented here.

ClickBench is the workload in it today. Forty three queries over one table, three runs each, the page cache dropped before each query's three, the first run reported as cold and the better of the second and third as hot. ClickBench does not publish one set of SQL, it publishes one per system, so this crate carries the DuckDB set, the DataFusion set and the ClickHouse reference the other two are rewrites of.

How far those rewrites go is worth knowing. The DataFusion set differs from the reference on forty two of the forty three, almost all of it quoting column names. The DuckDB set differs on two, and both are one function spelled a different way. Two out of forty three is small enough that somebody would be tempted to run the reference against DuckDB and not notice, which is why a test names those two queries and fails if the answer changes.

Dropping the page cache is privileged and does not work everywhere, so a run that could not do it says so and the label travels with the number. A warm first run published under the word cold is the single most misleading row a benchmark can print. Nothing here drops the cache itself, since that is the runner's job on a machine, and this crate only asks and records the answer it got.

Digests are taken here rather than inside a driver, because a driver that hashes its own output is a driver that can agree with itself about a wrong answer. Comparing them is the other half: two systems that disagree about what a query returns are not two systems whose speeds can be compared, and a query only one system answered is recorded as exactly that rather than counted as confirmed by somebody.

The published numbers are carried too, the same way and for the same reason. ClickBench keeps every result its leaderboard has shown as one file per system, per machine class, per date, and this crate holds the DuckDB and DataFusion files for c6a.4xlarge with their address and their digest. Putting our numbers next to those is the calibration gate for everything else here: a system that is much slower here than it is upstream is a system we configured wrong, and every comparison published afterwards would be measuring that mistake.

The hardware is not the same, so a ratio of one is not what correct looks like and neither is any other single number. What a faster machine does is move every query on every system by roughly one factor. So each driver gets its own ratio against the published numbers, the machine factor is the geometric mean of those ratios across the drivers, and a driver is out of band when its ratio sits more than 25 percent away from the shared factor. That catches one driver drifting, which is what a wrong thread count or a wrong memory limit looks like. It does not catch a mistake that slows everything here equally, which is indistinguishable from a slower machine, and that is said in the module rather than left for somebody to discover.

Part of [iris-bench](https://github.com/tamnd/iris-bench). Licensed under Apache-2.0.
