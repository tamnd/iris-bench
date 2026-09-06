//! The `DataFusion` driver.
//!
//! `DataFusion` is a query engine over files rather than a database with its own storage, so this
//! driver registers the corpus where it lies instead of ingesting it. That is what `DataFusion`'s
//! own published benchmark entries do, and it is a different trade from the one `DuckDB` makes.
//! Loading costs almost nothing here and the scan is paid for in the run phase, which is exactly
//! the difference the three phases exist to show. `CONFIG.md` says so in full.

use std::path::PathBuf;
use std::sync::Arc;

use bench_driver::{Answer, Driver, DriverError, Format, Load, Query, Rows, Setup, Value};
use datafusion::arrow::array::{Array, ArrayRef, AsArray};
use datafusion::arrow::datatypes::{
    DataType, Date32Type, Date64Type, Decimal128Type, Float32Type, Float64Type, Int8Type,
    Int16Type, Int32Type, Int64Type, Time32MillisecondType, Time32SecondType,
    Time64MicrosecondType, Time64NanosecondType, TimeUnit, TimestampMicrosecondType,
    TimestampMillisecondType, TimestampNanosecondType, TimestampSecondType, UInt8Type, UInt16Type,
    UInt32Type, UInt64Type,
};
use datafusion::datasource::file_format::csv::CsvFormat;
use datafusion::datasource::file_format::parquet::ParquetFormat;
use datafusion::datasource::listing::{
    ListingOptions, ListingTable, ListingTableConfig, ListingTableUrl,
};
use datafusion::error::Result as DataFusionResult;
use datafusion::execution::disk_manager::{DiskManagerBuilder, DiskManagerMode};
use datafusion::execution::runtime_env::RuntimeEnvBuilder;
use datafusion::prelude::{SessionConfig, SessionContext};
use tokio::runtime::Runtime;

/// The version of this crate, as reported by build metadata.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What [`Driver::version`] says before anything has been prepared.
///
/// A driver that has not been started cannot ask the system what version it is, and answering from
/// a constant in this file would defeat the point of asking.
const UNPREPARED: &str = "unprepared";

/// How much of the memory budget `DataFusion` is allowed to hand out.
///
/// All of it. The budget in [`Setup`] is already the whole allowance for the system under test, so
/// reserving a further slice here would quietly give `DataFusion` less than every other driver got.
const POOL_FRACTION: f64 = 1.0;

/// How many milliseconds a day is, for the one Arrow date type that counts in them.
const MILLISECONDS_PER_DAY: i64 = 24 * 60 * 60 * 1_000;

/// `DataFusion`, in process.
pub struct DataFusion {
    /// Built at preparation time, with the thread count the machine class asked for.
    runtime: Option<Runtime>,
    /// The session everything runs against, open once preparation has happened.
    context: Option<SessionContext>,
    /// What the system said its version was, read at preparation time.
    version: Option<String>,
}

impl std::fmt::Debug for DataFusion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Hand written because neither a tokio runtime nor a session context is Debug, and the two
        // things worth seeing here are whether it started and what it said it was.
        formatter
            .debug_struct("DataFusion")
            .field("runtime", &self.runtime.is_some())
            .field("context", &self.context.is_some())
            .field("version", &self.version)
            .finish()
    }
}

impl Default for DataFusion {
    fn default() -> Self {
        Self::new()
    }
}

impl DataFusion {
    /// A driver that has not been started yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            runtime: None,
            context: None,
            version: None,
        }
    }

    /// The runtime and the session, or a complaint that the phase order was not followed.
    fn started(&self) -> Result<(&Runtime, &SessionContext), DriverError> {
        match (self.runtime.as_ref(), self.context.as_ref()) {
            (Some(runtime), Some(context)) => Ok((runtime, context)),
            _ => Err(DriverError::system(
                "the session is not open",
                "prepare has not run, so there is nothing to talk to",
            )),
        }
    }
}

impl Driver for DataFusion {
    fn name(&self) -> &'static str {
        "datafusion"
    }

    fn version(&self) -> String {
        self.version
            .clone()
            .unwrap_or_else(|| UNPREPARED.to_owned())
    }

    fn prepare(&mut self, setup: &Setup) -> Result<(), DriverError> {
        // Both of these are set from the machine class rather than left to DataFusion, which sizes
        // its partitions from the host's core count and hands out memory without a ceiling. A
        // default that reads the host makes every result a result about that host.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(setup.threads)
            .enable_all()
            .build()
            .map_err(|error| DriverError::system("starting the runtime", error))?;

        let temp = setup.directory.join("temp");
        std::fs::create_dir_all(&temp)
            .map_err(|error| DriverError::system(format!("making {}", temp.display()), error))?;

        let environment = RuntimeEnvBuilder::new()
            .with_memory_limit(
                usize::try_from(setup.memory).unwrap_or(usize::MAX),
                POOL_FRACTION,
            )
            .with_disk_manager_builder(
                DiskManagerBuilder::default().with_mode(DiskManagerMode::Directories(vec![temp])),
            )
            .build_arc()
            .map_err(|error| DriverError::system("building the runtime environment", error))?;

        let config = SessionConfig::new().with_target_partitions(setup.threads);
        let context = SessionContext::new_with_config_rt(config, environment);

        let asked: DataFusionResult<Option<String>> = runtime.block_on(async {
            let frame = context.sql("SELECT version()").await?;
            let batches = frame.collect().await?;
            Ok(batches
                .first()
                .and_then(|batch| batch.column(0).as_string_opt::<i32>())
                .and_then(|column| column.iter().next().flatten())
                .map(str::to_owned))
        });
        let version = asked
            .map_err(|error| DriverError::system("asking DataFusion its version", error))?
            .ok_or_else(|| {
                DriverError::system(
                    "asking DataFusion its version",
                    "version() returned no rows",
                )
            })?;

        self.runtime = Some(runtime);
        self.context = Some(context);
        self.version = Some(version);
        Ok(())
    }

    fn load(&mut self, load: &Load) -> Result<(), DriverError> {
        let (runtime, context) = self.started()?;
        let options = options(load)?;
        let urls = urls(&load.files)?;

        // Registered where the files lie rather than copied into anything. See CONFIG.md: this is
        // DataFusion's published configuration, and it means the load phase records the schema
        // inference and nothing else while the run phase carries every scan.
        let registered: DataFusionResult<()> = runtime.block_on(async {
            let config = ListingTableConfig::new_with_multi_paths(urls)
                .with_listing_options(options)
                .infer_schema(&context.state())
                .await?;
            let table = ListingTable::try_new(config)?;
            context.register_table(load.table.as_str(), Arc::new(table))?;
            Ok(())
        });
        registered
            .map_err(|error| DriverError::system(format!("loading {}", load.table), error))?;
        Ok(())
    }

    fn run(&mut self, query: &Query) -> Result<Answer, DriverError> {
        let (runtime, context) = self.started()?;
        // collect rather than a stream that is dropped part way through. A lazy engine that
        // returned a plan here would have been timed on planning, and the comparison against a
        // system that actually computed the result would be worthless.
        let batches = runtime
            .block_on(async {
                let frame = context.sql(&query.sql).await?;
                frame.collect().await
            })
            .map_err(|error| DriverError::system(format!("running {}", query.id), error))?;

        let mut answer = Rows::new();
        for batch in &batches {
            for row in 0..batch.num_rows() {
                let mut values = Vec::with_capacity(batch.num_columns());
                for column in batch.columns() {
                    values.push(value(column, row, &query.id)?);
                }
                answer.push(values)?;
            }
        }
        Ok(answer.finish(query.ordered))
    }
}

/// How to read the files of one table.
fn options(load: &Load) -> Result<ListingOptions, DriverError> {
    Ok(match load.format {
        Format::Parquet => ListingOptions::new(Arc::new(ParquetFormat::default())),
        Format::Separated { separator } => {
            let delimiter = u8::try_from(separator).map_err(|_| {
                DriverError::unsupported(
                    load.table.clone(),
                    format!("{separator:?} is not a single byte, and DataFusion takes one byte"),
                )
            })?;
            // No header row, because the corpora in scope do not have one, and the column names
            // therefore come from DataFusion rather than from the workload. CONFIG.md records that
            // as the gap it is.
            let format = CsvFormat::default()
                .with_delimiter(delimiter)
                .with_has_header(false);
            ListingOptions::new(Arc::new(format))
        }
    })
}

/// The files of one table, as the urls a listing table takes.
///
/// Each file is named on its own rather than a directory being globbed, because the manifest says
/// which files a corpus is and a directory listing would say whatever happens to be on the disk.
fn urls(files: &[PathBuf]) -> Result<Vec<ListingTableUrl>, DriverError> {
    files
        .iter()
        .map(|file| {
            ListingTableUrl::parse(file.to_string_lossy())
                .map_err(|error| DriverError::system(format!("reading {}", file.display()), error))
        })
        .collect()
}

/// One value out of an Arrow array, in the canonical form.
///
/// A type this does not know about is an error rather than a guess. Rendering an unknown type the
/// way Arrow prints it would produce something that digests differently in every system while
/// looking like an answer, which is worse than saying the driver cannot render it.
#[allow(
    clippy::too_many_lines,
    reason = "one arm per Arrow type, and splitting it would only move the arms somewhere else"
)]
fn value(column: &ArrayRef, row: usize, query: &str) -> Result<Value, DriverError> {
    if column.is_null(row) {
        return Ok(Value::Null);
    }
    let unsupported = |what: &DataType| {
        DriverError::unsupported(
            format!("{query} column of type {what}"),
            "DataFusion returned a type this driver has no canonical form for".to_owned(),
        )
    };
    Ok(match column.data_type() {
        // A column whose whole type is null carries no validity buffer, so the check above does not
        // catch it and it has to be named.
        DataType::Null => Value::Null,
        DataType::Boolean => Value::Bool(column.as_boolean().value(row)),
        DataType::Int8 => Value::Int(i64::from(column.as_primitive::<Int8Type>().value(row))),
        DataType::Int16 => Value::Int(i64::from(column.as_primitive::<Int16Type>().value(row))),
        DataType::Int32 => Value::Int(i64::from(column.as_primitive::<Int32Type>().value(row))),
        DataType::Int64 => Value::Int(column.as_primitive::<Int64Type>().value(row)),
        DataType::UInt8 => Value::Int(i64::from(column.as_primitive::<UInt8Type>().value(row))),
        DataType::UInt16 => Value::Int(i64::from(column.as_primitive::<UInt16Type>().value(row))),
        DataType::UInt32 => Value::Int(i64::from(column.as_primitive::<UInt32Type>().value(row))),
        DataType::UInt64 => Value::Int(narrow(
            column.as_primitive::<UInt64Type>().value(row),
            query,
        )?),
        DataType::Float32 => {
            Value::Float(f64::from(column.as_primitive::<Float32Type>().value(row)))
        }
        DataType::Float64 => Value::Float(column.as_primitive::<Float64Type>().value(row)),
        DataType::Utf8 => Value::Text(column.as_string::<i32>().value(row).to_owned()),
        DataType::LargeUtf8 => Value::Text(column.as_string::<i64>().value(row).to_owned()),
        DataType::Utf8View => Value::Text(column.as_string_view().value(row).to_owned()),
        DataType::Date32 => Value::Date(column.as_primitive::<Date32Type>().value(row)),
        // Milliseconds since the epoch, which Arrow guarantees are a whole number of days, so this
        // divides rather than rounds.
        DataType::Date64 => {
            Value::Date(days(column.as_primitive::<Date64Type>().value(row), query)?)
        }
        DataType::Time32(TimeUnit::Second) => Value::Time(scale(
            i64::from(column.as_primitive::<Time32SecondType>().value(row)),
            1_000_000,
            query,
        )?),
        DataType::Time32(TimeUnit::Millisecond) => Value::Time(scale(
            i64::from(column.as_primitive::<Time32MillisecondType>().value(row)),
            1_000,
            query,
        )?),
        DataType::Time64(TimeUnit::Microsecond) => {
            Value::Time(column.as_primitive::<Time64MicrosecondType>().value(row))
        }
        DataType::Time64(TimeUnit::Nanosecond) => Value::Time(microseconds(
            column.as_primitive::<Time64NanosecondType>().value(row),
            query,
        )?),
        DataType::Timestamp(_, Some(_)) => {
            return Err(DriverError::unsupported(
                query.to_owned(),
                "a timestamp with a timezone has no canonical form here, because the canonical \
                 form has no timezone to render it in",
            ));
        }
        DataType::Timestamp(TimeUnit::Second, None) => Value::Timestamp(scale(
            column.as_primitive::<TimestampSecondType>().value(row),
            1_000_000,
            query,
        )?),
        DataType::Timestamp(TimeUnit::Millisecond, None) => Value::Timestamp(scale(
            column.as_primitive::<TimestampMillisecondType>().value(row),
            1_000,
            query,
        )?),
        DataType::Timestamp(TimeUnit::Microsecond, None) => {
            Value::Timestamp(column.as_primitive::<TimestampMicrosecondType>().value(row))
        }
        DataType::Timestamp(TimeUnit::Nanosecond, None) => Value::Timestamp(microseconds(
            column.as_primitive::<TimestampNanosecondType>().value(row),
            query,
        )?),
        // Rendered as a float rather than as exact digits, for the reason the DuckDB driver
        // renders its decimals as floats: the other systems in the matrix return the same
        // aggregate as a double, and a comparison only one of the three can pass is not a
        // comparison. CONFIG.md records it there as the deviation it is.
        DataType::Decimal128(_, digits) => Value::Float(decimal(
            column.as_primitive::<Decimal128Type>().value(row),
            *digits,
        )),
        other => return Err(unsupported(other)),
    })
}

/// A whole number of days out of a count of milliseconds.
fn days(milliseconds: i64, query: &str) -> Result<i32, DriverError> {
    i32::try_from(milliseconds.div_euclid(MILLISECONDS_PER_DAY)).map_err(|_| {
        DriverError::unsupported(
            query.to_owned(),
            format!("{milliseconds} milliseconds is further from the epoch than a date goes"),
        )
    })
}

/// A count in some coarser unit, in microseconds.
fn scale(inner: i64, factor: i64, query: &str) -> Result<i64, DriverError> {
    inner.checked_mul(factor).ok_or_else(|| {
        DriverError::unsupported(
            query.to_owned(),
            format!("{inner} does not fit in microseconds"),
        )
    })
}

/// A count of nanoseconds in microseconds, when that loses nothing.
///
/// Nanosecond is `DataFusion`'s default timestamp unit, so refusing it outright would refuse most
/// of what it returns, and dividing it down would make two instants that differ render the same. A
/// value that divides exactly is the same instant written in a coarser unit and is converted, and
/// one with a remainder is refused, which keeps the rule that the canonical form never merges two
/// values that are not equal.
fn microseconds(nanoseconds: i64, query: &str) -> Result<i64, DriverError> {
    if nanoseconds % 1_000 == 0 {
        Ok(nanoseconds / 1_000)
    } else {
        Err(DriverError::unsupported(
            query.to_owned(),
            format!("{nanoseconds} nanoseconds is finer than the canonical form goes"),
        ))
    }
}

/// A decimal as a double.
///
/// The scaled payload divided by ten to the scale, which is what the decimal means. Both halves
/// are exact in a double for every width these workloads use, and the canonical form rounds to six
/// significant digits anyway.
fn decimal(scaled: i128, digits: i8) -> f64 {
    #[allow(
        clippy::cast_precision_loss,
        reason = "an i128 payload wider than a double's mantissa is already past what six \
                  significant digits would keep"
    )]
    let payload = scaled as f64;
    payload / 10_f64.powi(i32::from(digits))
}

/// A wider integer as an `i64`, or a complaint.
///
/// No result in any workload here is larger than an `i64`, so one that is means the query is not
/// the one we think it is, and wrapping it silently would be the exact overflow the digest
/// comparison exists to catch.
fn narrow<T>(inner: T, query: &str) -> Result<i64, DriverError>
where
    T: Copy + std::fmt::Display + TryInto<i64>,
{
    inner.try_into().map_err(|_| {
        DriverError::unsupported(
            query.to_owned(),
            format!("{inner} does not fit in a signed 64 bit integer"),
        )
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use bench_driver::Session;

    use super::*;

    fn setup(directory: &Path) -> Setup {
        Setup {
            directory: directory.to_path_buf(),
            threads: 2,
            memory: 1 << 30,
        }
    }

    fn prepared(directory: &Path) -> DataFusion {
        let mut driver = DataFusion::new();
        driver.prepare(&setup(directory)).unwrap();
        driver
    }

    fn numbers(directory: &Path, name: &str, rows: &str) -> PathBuf {
        let file = directory.join(name);
        std::fs::write(&file, rows).unwrap();
        file
    }

    #[test]
    fn it_reports_all_three_phases_against_a_real_engine() {
        let scratch = tempfile::tempdir().unwrap();
        let table = numbers(scratch.path(), "numbers.csv", "1|one\n2|two\n3|three\n");

        let mut driver = DataFusion::new();
        let mut session = Session::new(&mut driver);

        session.prepare(&setup(scratch.path())).unwrap();
        session
            .load(&Load {
                table: "numbers".to_owned(),
                files: vec![table],
                format: Format::Separated { separator: '|' },
            })
            .unwrap();
        let (answer, _) = session
            .query(&Query {
                id: "q0".to_owned(),
                sql: "SELECT count(*), sum(column_1) FROM numbers".to_owned(),
                ordered: false,
            })
            .unwrap();

        assert_eq!(answer.body, "3\t6\n");
        let phases = session.phases();
        assert!(phases.prepare > 0.0 && phases.load > 0.0 && phases.run > 0.0);
        assert_eq!(phases.tables, 1);
        assert_eq!(phases.queries, 1);
    }

    #[test]
    fn it_reads_the_files_it_was_given_rather_than_the_directory_they_are_in() {
        // The manifest says what a corpus is. A directory says what happens to be on the disk, and
        // a stray file next to the corpus would otherwise become part of every answer.
        let scratch = tempfile::tempdir().unwrap();
        let first = numbers(scratch.path(), "part-0.csv", "1|one\n");
        let second = numbers(scratch.path(), "part-1.csv", "2|two\n");
        numbers(scratch.path(), "part-2.csv", "3|three\n");

        let mut driver = prepared(scratch.path());
        driver
            .load(&Load {
                table: "numbers".to_owned(),
                files: vec![first, second],
                format: Format::Separated { separator: '|' },
            })
            .unwrap();
        let answer = driver
            .run(&Query {
                id: "q0".to_owned(),
                sql: "SELECT sum(column_1) FROM numbers".to_owned(),
                ordered: false,
            })
            .unwrap();

        assert_eq!(answer.body, "3\n");
    }

    #[test]
    fn parquet_is_registered_where_it_lies_and_answered_from_there() {
        // Parquet is the form every corpus in scope is measured in, so the path that matters most
        // is exercised against a real file rather than only the text one.
        let scratch = tempfile::tempdir().unwrap();
        let file = scratch.path().join("numbers.parquet");
        let mut driver = prepared(scratch.path());
        driver
            .run(&Query {
                id: "write".to_owned(),
                sql: format!(
                    "COPY (SELECT 1 AS n UNION ALL SELECT 2 UNION ALL SELECT 3) TO '{}' \
                     STORED AS PARQUET",
                    file.display()
                ),
                ordered: true,
            })
            .unwrap();

        driver
            .load(&Load {
                table: "numbers".to_owned(),
                files: vec![file.clone()],
                format: Format::Parquet,
            })
            .unwrap();
        let answer = driver
            .run(&Query {
                id: "q0".to_owned(),
                sql: "SELECT count(*), sum(n) FROM numbers".to_owned(),
                ordered: false,
            })
            .unwrap();

        assert_eq!(answer.body, "3\t6\n");
        // Registered rather than ingested, so the file the corpus named is still the only copy.
        assert!(file.exists());
    }

    #[test]
    fn it_asks_the_system_what_version_it_is() {
        let scratch = tempfile::tempdir().unwrap();
        let mut driver = DataFusion::new();

        // Before preparation there is nothing to ask, and answering from a constant in this file
        // would defeat the point of asking.
        assert_eq!(driver.version(), UNPREPARED);

        driver.prepare(&setup(scratch.path())).unwrap();
        let version = driver.version();
        assert!(
            version.contains("DataFusion"),
            "{version} does not look like a DataFusion version"
        );
        assert_ne!(version, UNPREPARED);
    }

    #[test]
    fn the_settings_it_was_given_are_the_ones_in_force() {
        let scratch = tempfile::tempdir().unwrap();
        let driver = prepared(scratch.path());
        let (_, context) = driver.started().unwrap();

        // Read back off the session the engine is holding rather than off the builder this driver
        // used, so that a setting DataFusion quietly declined to take would show up here.
        let options = context.copied_config();
        assert_eq!(options.options().execution.target_partitions, 2);

        // Spilling goes somewhere known and inside the run's scratch directory, rather than
        // wherever the default put it.
        assert!(scratch.path().join("temp").is_dir());
    }

    #[test]
    fn every_type_it_can_return_has_a_canonical_form() {
        let scratch = tempfile::tempdir().unwrap();
        let mut driver = prepared(scratch.path());

        let answer = driver
            .run(&Query {
                id: "types".to_owned(),
                sql: "SELECT true, arrow_cast(1, 'Int8'), arrow_cast(2, 'Int64'), \
                      arrow_cast(3, 'UInt64'), arrow_cast(4.5, 'Float64'), \
                      arrow_cast(6.25, 'Decimal128(9, 2)'), 'text', NULL"
                    .to_owned(),
                ordered: true,
            })
            .unwrap();

        assert_eq!(answer.columns, 8);
        assert_eq!(answer.body, "true\t1\t2\t3\t4.5\t6.25\ttext\t\\N\n");
    }

    #[test]
    fn dates_and_timestamps_come_back_in_the_canonical_form_rather_than_arrows() {
        let scratch = tempfile::tempdir().unwrap();
        let mut driver = prepared(scratch.path());

        let answer = driver
            .run(&Query {
                id: "when".to_owned(),
                sql: "SELECT DATE '2024-02-29', \
                      arrow_cast(3723500000, 'Time64(Microsecond)'), \
                      TIMESTAMP '1998-12-01 13:45:00'"
                    .to_owned(),
                ordered: true,
            })
            .unwrap();

        assert_eq!(answer.body, "2024-02-29\t01:02:03.5\t1998-12-01 13:45:00\n");
    }

    #[test]
    fn a_nanosecond_timestamp_converts_when_it_can_and_says_so_when_it_cannot() {
        // Nanosecond is DataFusion's default timestamp unit, so refusing it outright would refuse
        // most of what it returns. Whole microseconds convert, a remainder does not.
        let scratch = tempfile::tempdir().unwrap();
        let mut driver = prepared(scratch.path());

        let answer = driver
            .run(&Query {
                id: "whole".to_owned(),
                sql: "SELECT arrow_cast(1000, 'Timestamp(Nanosecond, None)')".to_owned(),
                ordered: true,
            })
            .unwrap();
        assert_eq!(answer.body, "1970-01-01 00:00:00.000001\n");

        let error = driver
            .run(&Query {
                id: "fine".to_owned(),
                sql: "SELECT arrow_cast(1001, 'Timestamp(Nanosecond, None)')".to_owned(),
                ordered: true,
            })
            .unwrap_err();
        assert!(matches!(error, DriverError::Unsupported { .. }), "{error}");
    }

    #[test]
    fn an_integer_too_wide_for_the_canonical_form_is_refused_rather_than_wrapped() {
        let scratch = tempfile::tempdir().unwrap();
        let mut driver = prepared(scratch.path());

        let error = driver
            .run(&Query {
                id: "wide".to_owned(),
                sql: "SELECT arrow_cast('18446744073709551615', 'UInt64')".to_owned(),
                ordered: true,
            })
            .unwrap_err();
        assert!(error.to_string().contains("64 bit"), "{error}");
    }

    #[test]
    fn a_type_with_no_canonical_form_is_refused_rather_than_stringified() {
        // A list rendered as whatever Arrow prints would look like an answer and digest
        // differently in every system, which is worse than saying so.
        let scratch = tempfile::tempdir().unwrap();
        let mut driver = prepared(scratch.path());

        let error = driver
            .run(&Query {
                id: "list".to_owned(),
                sql: "SELECT make_array(1, 2)".to_owned(),
                ordered: true,
            })
            .unwrap_err();
        assert!(matches!(error, DriverError::Unsupported { .. }), "{error}");
    }

    #[test]
    fn a_query_it_cannot_parse_says_which_query() {
        let scratch = tempfile::tempdir().unwrap();
        let mut driver = prepared(scratch.path());

        let error = driver
            .run(&Query {
                id: "q41".to_owned(),
                sql: "SELECT FROM WHERE".to_owned(),
                ordered: false,
            })
            .unwrap_err();
        assert!(error.to_string().contains("q41"), "{error}");
    }

    #[test]
    fn it_names_itself_without_a_version_in_the_name() {
        assert_eq!(DataFusion::new().name(), "datafusion");
    }
}
