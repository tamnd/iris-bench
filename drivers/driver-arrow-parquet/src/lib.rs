//! The arrow-rs Parquet reader.
//!
//! This is the reference reader. It is not a query engine and does not pretend to be one: it reads
//! Parquet and projects columns, and anything else it is asked comes back as unsupported rather
//! than as a slow answer. It matters more than the other drivers because it is the baseline `iris`
//! will be compared against most often, and a badly configured version of it would flatter `iris`
//! in every table this repository ever prints.
//!
//! The Arrow to canonical mapping here is a near copy of the one in the `DataFusion` driver, which
//! looks like something that should be shared and is not. The two are on different versions of
//! Arrow, 58 by way of `DataFusion` and 59 here, so a shared crate could not serve both without
//! pinning one of them to the other's Arrow and quietly changing what that system is.

use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;

use arrow_array::cast::AsArray;
use arrow_array::{Array, ArrayRef, RecordBatch};
use arrow_schema::{DataType, Schema, TimeUnit};
use bench_driver::{Answer, Driver, DriverError, Format, Load, Query, Rows, Setup, Value};
use parquet::arrow::ProjectionMask;
use parquet::arrow::arrow_reader::{ArrowReaderOptions, ParquetRecordBatchReaderBuilder};
use parquet::file::metadata::PageIndexPolicy;
use parquet::file::properties::DEFAULT_CREATED_BY;

/// The version of this crate, as reported by build metadata.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// How many rows come back from the reader at a time.
///
/// arrow-rs defaults to 1024 and this is what `DataFusion` asks it for, which is the number the
/// reader is tuned and reported against by the people who maintain it. Picking our own here would
/// be this repository tuning the baseline that everything else is measured against, which is the
/// one baseline where our own guesswork does the most damage.
const BATCH_SIZE: usize = 8192;

/// Whether the page index is read at open time.
///
/// Skipped. The page index earns its keep when a predicate or a limit can use it to skip pages, and
/// this driver has neither, so reading it would be time spent on a structure nothing here consults.
/// `CONFIG.md` says what would have to change for that to be the wrong answer.
const PAGE_INDEX: PageIndexPolicy = PageIndexPolicy::Skip;

/// The arrow-rs Parquet reader.
#[derive(Debug, Default)]
pub struct ArrowParquet {
    /// What each table's files are, in the order the manifest named them.
    tables: HashMap<String, Vec<PathBuf>>,
    /// Whether preparation has run.
    prepared: bool,
}

impl ArrowParquet {
    /// A driver that has not been started yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The files of one table, or a complaint that it was never loaded.
    fn files(&self, table: &str) -> Result<&[PathBuf], DriverError> {
        self.tables
            .get(table)
            .map(Vec::as_slice)
            .ok_or_else(|| DriverError::unsupported(table.to_owned(), "no such table is loaded"))
    }
}

impl Driver for ArrowParquet {
    fn name(&self) -> &'static str {
        "arrow-parquet"
    }

    fn version(&self) -> String {
        // The string the library stamps into every file it writes, which is the library reporting
        // its own version rather than this driver reporting a constant somebody typed.
        DEFAULT_CREATED_BY.to_owned()
    }

    fn prepare(&mut self, setup: &Setup) -> Result<(), DriverError> {
        // There is no engine here to configure and nothing to start. What preparation does is make
        // the scratch directory and say so, and pretending otherwise by inventing work would put a
        // number in the prepare column that means nothing.
        std::fs::create_dir_all(&setup.directory).map_err(|error| {
            DriverError::system(format!("making {}", setup.directory.display()), error)
        })?;
        self.prepared = true;
        Ok(())
    }

    fn load(&mut self, load: &Load) -> Result<(), DriverError> {
        if !self.prepared {
            return Err(DriverError::system(
                "nothing is prepared",
                "prepare has not run, so there is no scratch directory",
            ));
        }
        if load.format != Format::Parquet {
            return Err(DriverError::unsupported(
                load.table.clone(),
                "this driver reads Parquet and nothing else, which is what makes it the reference \
                 reader rather than an engine",
            ));
        }
        if load.projection.is_some() {
            return Err(DriverError::unsupported(
                load.table.clone(),
                "this driver reads the columns the files store and has no SQL to apply a select \
                 list with, so a workload that needs one has to be answered by an engine",
            ));
        }
        // Every file is opened and its footer read, so that a file which is not Parquet or whose
        // schema disagrees with the rest fails here rather than part way through a measurement.
        let mut schema: Option<Schema> = None;
        for file in &load.files {
            let found = footer(file)?;
            if let Some(first) = &schema {
                if first.fields() != found.fields() {
                    return Err(DriverError::unsupported(
                        load.table.clone(),
                        format!(
                            "{} has a different schema from the files before it",
                            file.display()
                        ),
                    ));
                }
            } else {
                schema = Some(found);
            }
        }
        self.tables.insert(load.table.clone(), load.files.clone());
        Ok(())
    }

    fn run(&mut self, query: &Query) -> Result<Answer, DriverError> {
        let scan = scan(&query.sql, &query.id)?;
        let files = self.files(&scan.table)?;
        let mut answer = Rows::new();

        for path in files {
            let file = File::open(path).map_err(|error| {
                DriverError::system(format!("opening {}", path.display()), error)
            })?;
            let options = ArrowReaderOptions::new().with_page_index_policy(PAGE_INDEX);
            let builder = ParquetRecordBatchReaderBuilder::try_new_with_options(file, options)
                .map_err(|error| {
                    DriverError::system(format!("reading {}", path.display()), error)
                })?;

            let wanted = order(builder.schema(), scan.columns.as_deref(), &query.id)?;
            let mask = ProjectionMask::roots(builder.parquet_schema(), wanted.clone());
            let reader = builder
                .with_batch_size(BATCH_SIZE)
                .with_projection(mask)
                .build()
                .map_err(|error| {
                    DriverError::system(format!("opening {}", path.display()), error)
                })?;

            for batch in reader {
                let batch = batch.map_err(|error| {
                    DriverError::system(format!("decoding {}", path.display()), error)
                })?;
                // The mask does not reorder, so the requested order is applied here rather than
                // being quietly replaced by the order the file happens to store.
                let columns = project(&batch, &wanted, &query.id)?;
                for row in 0..batch.num_rows() {
                    let mut values = Vec::with_capacity(columns.len());
                    for column in &columns {
                        values.push(value(column, row, &query.id)?);
                    }
                    answer.push(values)?;
                }
            }
        }
        Ok(answer.finish(query.ordered))
    }
}

/// What one query asks for.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Scan {
    /// The columns, in the order they were asked for, or every column in file order.
    columns: Option<Vec<String>>,
    /// The table.
    table: String,
}

/// Reads the one shape of query this driver can answer.
///
/// `SELECT * FROM t` and `SELECT a, b FROM t`, and nothing else. A predicate, a join or an
/// aggregate is refused by name rather than half answered, so that the result says the reader
/// cannot express the query rather than reporting a fast time against something else.
fn scan(sql: &str, query: &str) -> Result<Scan, DriverError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let lowered = trimmed.to_lowercase();
    let refuse = |what: &str| {
        DriverError::unsupported(
            query.to_owned(),
            format!("this driver reads Parquet and projects columns, and {what}"),
        )
    };

    // Positions are found in the lowercased copy and then used on the original, which only lines up
    // while the two are the same length. Every query in scope is ASCII and one that is not gets
    // said so rather than sliced at an offset that means something else.
    if !trimmed.is_ascii() {
        return Err(refuse("this is not plain ASCII SQL"));
    }
    if !lowered.starts_with("select ") {
        return Err(refuse("this is not a select"));
    }
    let Some(split) = lowered.find(" from ") else {
        return Err(refuse("this has no from clause"));
    };
    let list = trimmed["select ".len()..split].trim();
    let after = trimmed[split + " from ".len()..].trim();

    let mut words = after.split_whitespace();
    let Some(table) = words.next() else {
        return Err(refuse("this names no table"));
    };
    if let Some(extra) = words.next() {
        return Err(refuse(&format!("{extra} is more than it can do")));
    }

    let columns = if list == "*" {
        None
    } else {
        let mut named = Vec::new();
        for part in list.split(',') {
            let name = part.trim().trim_matches('"').trim();
            if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return Err(refuse(&format!("{name} is not a plain column name")));
            }
            named.push(name.to_owned());
        }
        Some(named)
    };
    Ok(Scan {
        columns,
        table: table.trim_matches('"').to_owned(),
    })
}

/// Which columns of a file are wanted, as indices into its schema, in the order asked for.
fn order(
    schema: &Schema,
    columns: Option<&[String]>,
    query: &str,
) -> Result<Vec<usize>, DriverError> {
    match columns {
        None => Ok((0..schema.fields().len()).collect()),
        Some(named) => named
            .iter()
            .map(|name| {
                schema.index_of(name).map_err(|_| {
                    DriverError::unsupported(
                        query.to_owned(),
                        format!("there is no column called {name} in this table"),
                    )
                })
            })
            .collect(),
    }
}

/// The columns of one batch, in the order the query asked for them.
///
/// A projected batch holds the wanted columns in file order, so this maps each requested position
/// back onto it rather than assuming the two agree.
fn project(
    batch: &RecordBatch,
    wanted: &[usize],
    query: &str,
) -> Result<Vec<ArrayRef>, DriverError> {
    let mut sorted: Vec<usize> = wanted.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    wanted
        .iter()
        .map(|position| {
            let found = sorted.binary_search(position).map_err(|_| {
                DriverError::system(query.to_owned(), "a projected column went missing")
            })?;
            batch.columns().get(found).cloned().ok_or_else(|| {
                DriverError::system(
                    query.to_owned(),
                    "the batch is narrower than the projection",
                )
            })
        })
        .collect()
}

/// The Arrow schema of one file, read from its footer.
fn footer(path: &PathBuf) -> Result<Schema, DriverError> {
    let file = File::open(path)
        .map_err(|error| DriverError::system(format!("opening {}", path.display()), error))?;
    let options = ArrowReaderOptions::new().with_page_index_policy(PAGE_INDEX);
    let builder = ParquetRecordBatchReaderBuilder::try_new_with_options(file, options)
        .map_err(|error| DriverError::system(format!("reading {}", path.display()), error))?;
    Ok(builder.schema().as_ref().clone())
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
    use arrow_array::types::{
        Date32Type, Date64Type, Decimal128Type, Float32Type, Float64Type, Int8Type, Int16Type,
        Int32Type, Int64Type, Time32MillisecondType, Time32SecondType, Time64MicrosecondType,
        Time64NanosecondType, TimestampMicrosecondType, TimestampMillisecondType,
        TimestampNanosecondType, TimestampSecondType, UInt8Type, UInt16Type, UInt32Type,
        UInt64Type,
    };

    if column.is_null(row) {
        return Ok(Value::Null);
    }
    let unsupported = |what: &DataType| {
        DriverError::unsupported(
            query.to_owned(),
            format!("a column of type {what} has no canonical form here"),
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
        // Rendered as a float rather than as exact digits, for the reason the other two drivers
        // render their decimals as floats: they return the same aggregate as a double, and a
        // comparison only one of the three can pass is not a comparison.
        DataType::Decimal128(_, digits) => Value::Float(decimal(
            column.as_primitive::<Decimal128Type>().value(row),
            *digits,
        )),
        other => return Err(unsupported(other)),
    })
}

/// A wider integer as an `i64`, or a complaint.
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

/// A whole number of days out of a count of milliseconds.
fn days(milliseconds: i64, query: &str) -> Result<i32, DriverError> {
    i32::try_from(milliseconds.div_euclid(24 * 60 * 60 * 1_000)).map_err(|_| {
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
/// A value that divides exactly is the same instant written in a coarser unit and is converted, and
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
fn decimal(scaled: i128, digits: i8) -> f64 {
    #[allow(
        clippy::cast_precision_loss,
        reason = "an i128 payload wider than a double's mantissa is already past what six \
                  significant digits would keep"
    )]
    let payload = scaled as f64;
    payload / 10_f64.powi(i32::from(digits))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_array::{Int64Array, StringArray};
    use arrow_schema::Field;
    use bench_driver::Session;
    use parquet::arrow::ArrowWriter;

    use super::*;

    fn setup(directory: &std::path::Path) -> Setup {
        Setup {
            directory: directory.to_path_buf(),
            threads: 2,
            memory: 1 << 30,
        }
    }

    /// A two column Parquet file, written with the same library that reads it back.
    fn written(directory: &std::path::Path, name: &str, numbers: &[i64]) -> PathBuf {
        let schema = Arc::new(Schema::new(vec![
            Field::new("n", DataType::Int64, false),
            Field::new("word", DataType::Utf8, false),
        ]));
        let words: Vec<String> = numbers.iter().map(|n| format!("row {n}")).collect();
        let batch = RecordBatch::try_new(
            Arc::clone(&schema),
            vec![
                Arc::new(Int64Array::from(numbers.to_vec())),
                Arc::new(StringArray::from(words)),
            ],
        )
        .unwrap();
        let path = directory.join(name);
        let file = File::create(&path).unwrap();
        let mut writer = ArrowWriter::try_new(file, schema, None).unwrap();
        writer.write(&batch).unwrap();
        writer.close().unwrap();
        path
    }

    fn loaded(directory: &std::path::Path, files: Vec<PathBuf>) -> ArrowParquet {
        let mut driver = ArrowParquet::new();
        driver.prepare(&setup(directory)).unwrap();
        driver
            .load(&Load {
                table: "numbers".to_owned(),
                files,
                format: Format::Parquet,
                projection: None,
            })
            .unwrap();
        driver
    }

    #[test]
    fn it_reports_all_three_phases_against_a_real_file() {
        let scratch = tempfile::tempdir().unwrap();
        let file = written(scratch.path(), "numbers.parquet", &[1, 2, 3]);

        let mut driver = ArrowParquet::new();
        let mut session = Session::new(&mut driver);
        session.prepare(&setup(scratch.path())).unwrap();
        session
            .load(&Load {
                table: "numbers".to_owned(),
                files: vec![file],
                format: Format::Parquet,
                projection: None,
            })
            .unwrap();
        let (answer, _) = session
            .query(&Query {
                id: "q0".to_owned(),
                sql: "SELECT n FROM numbers".to_owned(),
                ordered: false,
            })
            .unwrap();

        assert_eq!(answer.body, "1\n2\n3\n");
        let phases = session.phases();
        assert!(phases.prepare > 0.0 && phases.load > 0.0 && phases.run > 0.0);
        assert_eq!(phases.tables, 1);
        assert_eq!(phases.queries, 1);
    }

    #[test]
    fn a_projection_comes_back_in_the_order_it_was_asked_for() {
        // The projection mask does not reorder, so a driver that trusted it would answer a
        // different question from the one the query asked and digest as though it had not.
        let scratch = tempfile::tempdir().unwrap();
        let file = written(scratch.path(), "numbers.parquet", &[1]);
        let mut driver = loaded(scratch.path(), vec![file]);

        let answer = driver
            .run(&Query {
                id: "q0".to_owned(),
                sql: "SELECT word, n FROM numbers".to_owned(),
                ordered: true,
            })
            .unwrap();
        assert_eq!(answer.body, "row 1\t1\n");
    }

    #[test]
    fn a_star_reads_every_column_in_file_order() {
        let scratch = tempfile::tempdir().unwrap();
        let file = written(scratch.path(), "numbers.parquet", &[7]);
        let mut driver = loaded(scratch.path(), vec![file]);

        let answer = driver
            .run(&Query {
                id: "q0".to_owned(),
                sql: "SELECT * FROM numbers".to_owned(),
                ordered: true,
            })
            .unwrap();
        assert_eq!(answer.columns, 2);
        assert_eq!(answer.body, "7\trow 7\n");
    }

    #[test]
    fn a_table_of_several_files_reads_all_of_them() {
        let scratch = tempfile::tempdir().unwrap();
        let first = written(scratch.path(), "part-0.parquet", &[1, 2]);
        let second = written(scratch.path(), "part-1.parquet", &[3]);
        let mut driver = loaded(scratch.path(), vec![first, second]);

        let answer = driver
            .run(&Query {
                id: "q0".to_owned(),
                sql: "SELECT n FROM numbers".to_owned(),
                ordered: false,
            })
            .unwrap();
        assert_eq!(answer.rows, 3);
        assert_eq!(answer.body, "1\n2\n3\n");
    }

    #[test]
    fn a_query_it_cannot_express_is_unsupported_rather_than_wrong() {
        // The point of the whole DriverError::Unsupported path. A reference reader that quietly
        // answered an aggregate with a scan would put a fast time next to a wrong result.
        let scratch = tempfile::tempdir().unwrap();
        let file = written(scratch.path(), "numbers.parquet", &[1]);
        let mut driver = loaded(scratch.path(), vec![file]);

        for sql in [
            "SELECT count(*) FROM numbers",
            "SELECT n FROM numbers WHERE n > 1",
            "SELECT n FROM numbers ORDER BY n",
            "SELECT n + 1 FROM numbers",
            "INSERT INTO numbers VALUES (1)",
        ] {
            let error = driver
                .run(&Query {
                    id: "q0".to_owned(),
                    sql: sql.to_owned(),
                    ordered: false,
                })
                .unwrap_err();
            assert!(
                matches!(error, DriverError::Unsupported { .. }),
                "{sql} gave {error}"
            );
        }
    }

    #[test]
    fn a_column_that_is_not_there_says_so_rather_than_returning_nothing() {
        let scratch = tempfile::tempdir().unwrap();
        let file = written(scratch.path(), "numbers.parquet", &[1]);
        let mut driver = loaded(scratch.path(), vec![file]);

        let error = driver
            .run(&Query {
                id: "q0".to_owned(),
                sql: "SELECT missing FROM numbers".to_owned(),
                ordered: false,
            })
            .unwrap_err();
        assert!(error.to_string().contains("missing"), "{error}");
    }

    #[test]
    fn text_is_the_one_thing_it_reads_and_a_csv_is_refused() {
        // It is the reference Parquet reader. A driver that also read CSV would be an engine with
        // a Parquet reader in it, which is a different thing to be the baseline for.
        let scratch = tempfile::tempdir().unwrap();
        let text = scratch.path().join("numbers.csv");
        std::fs::write(&text, "1|one\n").unwrap();

        let mut driver = ArrowParquet::new();
        driver.prepare(&setup(scratch.path())).unwrap();
        let error = driver
            .load(&Load {
                table: "numbers".to_owned(),
                files: vec![text],
                format: Format::Separated { separator: '|' },
                projection: None,
            })
            .unwrap_err();
        assert!(matches!(error, DriverError::Unsupported { .. }), "{error}");
    }

    #[test]
    fn a_select_list_is_refused_rather_than_ignored() {
        // Nothing hands this driver a projection today and the workload says in one place why not.
        // Ignoring one would still be the wrong answer, because a reader that quietly skipped the
        // conversion every other system applied would be answering a different question and
        // reporting a time for it.
        let scratch = tempfile::tempdir().unwrap();
        let file = written(scratch.path(), "numbers.parquet", &[1, 2, 3]);

        let mut driver = ArrowParquet::new();
        driver.prepare(&setup(scratch.path())).unwrap();
        let error = driver
            .load(&Load {
                table: "numbers".to_owned(),
                files: vec![file],
                format: Format::Parquet,
                projection: Some("* REPLACE (make_date(n) AS n)".to_owned()),
            })
            .unwrap_err();
        assert!(matches!(error, DriverError::Unsupported { .. }), "{error}");
    }

    #[test]
    fn a_file_that_is_not_parquet_fails_at_load_rather_than_part_way_through_a_measurement() {
        let scratch = tempfile::tempdir().unwrap();
        let fake = scratch.path().join("numbers.parquet");
        std::fs::write(&fake, "this is not a parquet file").unwrap();

        let mut driver = ArrowParquet::new();
        driver.prepare(&setup(scratch.path())).unwrap();
        let error = driver
            .load(&Load {
                table: "numbers".to_owned(),
                files: vec![fake],
                format: Format::Parquet,
                projection: None,
            })
            .unwrap_err();
        assert!(error.to_string().contains("numbers.parquet"), "{error}");
    }

    #[test]
    fn it_reports_the_version_the_library_stamps_into_its_own_files() {
        let version = ArrowParquet::new().version();
        assert!(version.contains("parquet-rs"), "{version}");
    }

    #[test]
    fn it_names_itself_without_a_version_in_the_name() {
        assert_eq!(ArrowParquet::new().name(), "arrow-parquet");
    }
}
