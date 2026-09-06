# driver-arrow-parquet

The arrow-rs Parquet reader.

This is the reference reader, and it is not a query engine. It reads Parquet and projects columns, and anything else it is asked comes back as unsupported rather than as a slow answer to a different question. It matters more than the other drivers because it is the baseline iris will be compared against most often, so a badly configured version of it would flatter iris in every table this repository ever prints.

`CONFIG.md` names the batch size, the page index policy, the codecs and the row group behaviour, along with the two things this driver does not do that the other two do: it ignores the thread count, because arrow-rs's reader is single threaded, and it does not enforce the memory budget, because there is no pool to enforce it with. Both are written down rather than papered over, since both make its run column mean something different from theirs.

Part of [iris-bench](https://github.com/tamnd/iris-bench). Licensed under Apache-2.0.
