# bench-driver

The trait every system under test implements.

The prepare, load and run split is fixed here for everyone, so that no

system can move work into an untimed phase that another system pays for.

Part of [iris-bench](https://github.com/tamnd/iris-bench). Licensed under Apache-2.0.
