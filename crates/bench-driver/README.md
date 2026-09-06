# bench-driver

The trait every system under test implements.

The prepare, load and run split is fixed here for everyone, so that no system can move work into an untimed phase that another system pays for. A driver implements three methods and none of them times itself. A session borrows the driver, calls the three methods, and holds the clock.

Preparation is given no file paths, so a load cannot hide inside it and no index can be built there. A query cannot be answered before something has been loaded, because a system that could answer one read the data during preparation. Both of those are the mechanism rather than a convention.

Result rendering is defined here too, once, so that no driver invents its own. Two of the rules in it cost something and `docs/METHODOLOGY.md` says what.

Part of [iris-bench](https://github.com/tamnd/iris-bench). Licensed under Apache-2.0.
