# driver-duckdb

The DuckDB driver.

Configured from DuckDB's own performance guide rather than from whatever the defaults happen to be on the machine. Threads, memory limit and temp directory come from the machine class, because a default that reads the host makes every result a result about that host. `CONFIG.md` names every setting, where it came from, the two places this driver departs from what DuckDB actually computed, and why.

DuckDB is linked in bundled, so the version under test is fixed by this workspace's lockfile rather than by what each machine had installed.

Part of [iris-bench](https://github.com/tamnd/iris-bench). Licensed under Apache-2.0.
