# driver-arrow-parquet configuration

This is the reference reader, so this file matters more than the other two. Every claim `iris` ever makes about being faster than Parquet is a claim about the numbers this driver produces, and a reader configured badly here would flatter `iris` in every table this repository prints. Read it as an adversary would.

## Source

The arrow-rs Parquet crate's own documentation for `ArrowReaderOptions`, `ParquetRecordBatchReaderBuilder` and `ProjectionMask`, and the settings DataFusion asks that same reader for in its published benchmark entries, since DataFusion is the largest user of it and the configuration its maintainers see numbers from.

The version under test is fixed by this workspace's lockfile, since arrow-rs is a Rust library compiled into this binary. The driver reports the string the library stamps into every file it writes, `parquet-rs version x.y.z`, which is the library naming itself rather than this driver repeating a constant somebody typed.

## Settings

The batch size is 8192 rows, which is what DataFusion asks this reader for. The crate's own default is 1024. Picking a third number here would be this repository tuning the one baseline where its own guesswork does the most damage, so the number comes from the largest published user of the reader rather than from us.

The page index is skipped. It is read so that a predicate or a limit can use it to skip pages, and this driver has neither, so reading it would be time spent on a structure nothing here consults. If this driver ever gains predicate pushdown, this is the first setting that has to change, and a run that skipped the index while pushing down a predicate would be measuring a reader nobody would deploy.

Every row group is read. There is no row group filtering because there is nothing to filter on, and a reader that skipped row groups without a predicate would be answering a different question.

Columns are projected with a `ProjectionMask` built from the query, so a query naming two columns reads two columns off the disk. The mask does not reorder, so the requested order is applied after decoding rather than being quietly replaced by the order the file stores, and there is a test for that.

Codecs are the full set arrow-rs turns on by default: snap, brotli, gzip through flate2, lz4 and zstd. The workspace turns parquet's default features off so that a crate which only reads footers does not pull in every codec, so this driver names them one by one. A reference reader that could not decompress a page would score well on the subset of files it happened to be able to read.

## Deviations

This driver ignores the thread count it is given. arrow-rs's synchronous reader is single threaded, and the parallel path over it is the one DataFusion builds, not one arrow-rs publishes. The number in the run column here is therefore a single core number and the same number from DuckDB and DataFusion is not, and comparing the two directly would say more about core counts than about readers. It is the right measurement for what this driver is for, which is decode against decode, and it is the wrong measurement for a wall clock race. The thread count is still recorded in the result row so a reader can see it was ignored.

The memory budget is not enforced. There is no pool to set a ceiling on, and the reader's footprint is a batch at a time plus whatever the decoder holds. A run that exceeded the budget would be stopped by the machine rather than by the reader, which is a real difference from the other two drivers and is recorded rather than papered over.

It answers one shape of query. `SELECT * FROM t` and `SELECT a, b FROM t`, and nothing else. A predicate, an aggregate, a join or an order by comes back as unsupported, recorded as unsupported rather than as a failure or a fast time against something that was not asked. That is not a limitation of arrow-rs, it is what arrow-rs is: a reader rather than an engine. The alternative was writing a query engine in this driver, which would then be the thing being measured.

It reads Parquet and refuses text. A driver that also read CSV would be an engine with a Parquet reader inside it, which is a different thing to be the baseline for.

The Arrow to canonical value mapping here is a near copy of the one in the DataFusion driver. The two are on different versions of Arrow, 58 by way of DataFusion and 59 here, so a shared crate could not serve both without pinning one of them to the other's Arrow and quietly changing what that system is.

## Rejected alternatives

The asynchronous reader, `ParquetRecordBatchStreamBuilder`, was rejected. It is the right choice against object storage and it would put a runtime and a concurrency setting between the measurement and the decoder, which is exactly what this driver exists not to do. Every corpus here is already on local disk.

Reading files in parallel with a thread pool was rejected, for the reason in the deviations above. It would make the number look better against the other two drivers and it would be this repository's own parallel reader being measured rather than arrow-rs's.

Setting the batch size to something larger, which does make a scan faster, was rejected. It is the single easiest way to make the baseline look worse than it is by leaving it at 1024, and the single easiest way to make it look artificially good by tuning it upward until the numbers stop improving. Taking it from DataFusion is the only choice here that nobody in this repository made.

Answering `SELECT count(*)` from the footer's row count was rejected. It is what any reader would really do and it would take no time at all, which makes it a measurement of reading a footer sitting in a column that readers of the table would take for a scan.

## Last reviewed

2026-09-06
