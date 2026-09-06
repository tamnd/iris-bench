//! The `DuckDB` driver.
//!
//! Configured from `DuckDB`'s own performance guide rather than from whatever the defaults happen to
//! be on the machine, and `CONFIG.md` names every setting, where it came from, and the one place we
//! departed from the guide and why.
//!
//! `DuckDB` is linked in bundled, so the version under test is fixed by this workspace's lockfile
//! rather than by what each machine had installed. A version that varies by machine is a version
//! nobody pinned, and it goes into every result row.

use std::path::Path;

use bench_driver::{Answer, Driver, DriverError, Format, Load, Query, Rows, Setup, Value};
use duckdb::Connection;
use duckdb::types::ValueRef;

/// The version of this crate, as reported by build metadata.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What [`Driver::version`] says before anything has been prepared.
///
/// A driver that has not been started cannot ask the system what version it is, and inventing an
/// answer from a constant in this file would defeat the point of asking.
const UNPREPARED: &str = "unprepared";

/// `DuckDB`, in process.
#[derive(Debug, Default)]
pub struct DuckDb {
    /// Open once preparation has happened.
    connection: Option<Connection>,
    /// What the system said its version was, read at preparation time.
    version: Option<String>,
}

impl DuckDb {
    /// A driver that has not been started yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The open connection, or a complaint that the phase order was not followed.
    fn connection(&self) -> Result<&Connection, DriverError> {
        self.connection.as_ref().ok_or_else(|| {
            DriverError::system(
                "the connection is not open",
                "prepare has not run, so there is nothing to talk to",
            )
        })
    }
}

impl Driver for DuckDb {
    fn name(&self) -> &'static str {
        "duckdb"
    }

    fn version(&self) -> String {
        self.version
            .clone()
            .unwrap_or_else(|| UNPREPARED.to_owned())
    }

    fn prepare(&mut self, setup: &Setup) -> Result<(), DriverError> {
        // A file rather than in memory. DuckDB's performance guide is written around a persistent
        // database, and an in memory one measures a configuration nobody deploys.
        let database = setup.directory.join("duckdb.db");
        let connection = Connection::open(&database).map_err(|error| {
            DriverError::system(format!("opening {}", database.display()), error)
        })?;

        let temp = setup.directory.join("temp");
        std::fs::create_dir_all(&temp)
            .map_err(|error| DriverError::system(format!("making {}", temp.display()), error))?;

        // Every one of these is in CONFIG.md with the page it came from. Nothing here is left at a
        // default that would read the host's core count or its free memory, because a default that
        // reads the host makes every result a result about that host.
        set(&connection, "threads", &setup.threads.to_string())?;
        set(&connection, "memory_limit", &format!("'{}B'", setup.memory))?;
        set(&connection, "temp_directory", &quoted(&temp))?;
        set(&connection, "preserve_insertion_order", "false")?;

        let version: String = connection
            .query_row("SELECT version()", [], |row| row.get(0))
            .map_err(|error| DriverError::system("asking DuckDB its version", error))?;

        self.connection = Some(connection);
        self.version = Some(version);
        Ok(())
    }

    fn load(&mut self, load: &Load) -> Result<(), DriverError> {
        let connection = self.connection()?;
        let files = list(&load.files);
        let reader = match load.format {
            Format::Parquet => format!("read_parquet({files})"),
            Format::Separated { separator } => format!(
                "read_csv({files}, delim = '{}', header = false, auto_detect = true)",
                escaped(separator)
            ),
        };
        // CREATE TABLE AS rather than a view over the files. Reading in place would be a legitimate
        // thing for a driver to do, but it would move the work into the run phase, and DuckDB's own
        // published ClickBench entry loads into a table. The load timing is where that choice shows
        // up, which is the whole reason the phase exists.
        let statement = format!(
            "CREATE OR REPLACE TABLE {} AS SELECT * FROM {reader}",
            load.table
        );
        connection
            .execute_batch(&statement)
            .map_err(|error| DriverError::system(format!("loading {}", load.table), error))?;
        Ok(())
    }

    fn run(&mut self, query: &Query) -> Result<Answer, DriverError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(&query.sql)
            .map_err(|error| DriverError::system(format!("preparing {}", query.id), error))?;
        let mut answer = Rows::new();
        let mut rows = statement
            .query([])
            .map_err(|error| DriverError::system(format!("running {}", query.id), error))?;

        // Every row is pulled and rendered here, which is what makes this a measurement of the
        // query rather than of building a plan.
        while let Some(row) = rows
            .next()
            .map_err(|error| DriverError::system(format!("reading {}", query.id), error))?
        {
            let mut values = Vec::new();
            for column in 0.. {
                let Ok(raw) = row.get_ref(column) else { break };
                values.push(value(raw, &query.id, column)?);
            }
            answer.push(values)?;
        }
        Ok(answer.finish(query.ordered))
    }
}

/// Applies one setting, and says which one if it will not take.
fn set(connection: &Connection, name: &str, value: &str) -> Result<(), DriverError> {
    connection
        .execute_batch(&format!("SET {name} = {value}"))
        .map_err(|error| DriverError::system(format!("setting {name} to {value}"), error))
}

/// A path as a single quoted SQL string.
fn quoted(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "''"))
}

/// A list of paths as a SQL list, which is what `read_parquet` and `read_csv` both take.
fn list(files: &[std::path::PathBuf]) -> String {
    let inside: Vec<String> = files.iter().map(|file| quoted(file)).collect();
    format!("[{}]", inside.join(", "))
}

/// A separator inside a single quoted SQL string.
fn escaped(separator: char) -> String {
    if separator == '\'' {
        "''".to_owned()
    } else {
        separator.to_string()
    }
}

/// One `DuckDB` value in the canonical form.
///
/// A type this does not know about is an error rather than a guess. Stringifying an unknown type
/// would produce something that digests differently in every system while looking like an answer,
/// which is worse than saying the driver cannot render it.
fn value(raw: ValueRef<'_>, query: &str, column: usize) -> Result<Value, DriverError> {
    Ok(match raw {
        ValueRef::Null => Value::Null,
        ValueRef::Boolean(inner) => Value::Bool(inner),
        ValueRef::TinyInt(inner) => Value::Int(i64::from(inner)),
        ValueRef::SmallInt(inner) => Value::Int(i64::from(inner)),
        ValueRef::Int(inner) => Value::Int(i64::from(inner)),
        ValueRef::BigInt(inner) => Value::Int(inner),
        ValueRef::UTinyInt(inner) => Value::Int(i64::from(inner)),
        ValueRef::USmallInt(inner) => Value::Int(i64::from(inner)),
        ValueRef::UInt(inner) => Value::Int(i64::from(inner)),
        ValueRef::UBigInt(inner) => Value::Int(narrow(inner, query, column)?),
        ValueRef::HugeInt(inner) => Value::Int(narrow(inner, query, column)?),
        ValueRef::UHugeInt(inner) => Value::Int(narrow(inner, query, column)?),
        ValueRef::Float(inner) => Value::Float(f64::from(inner)),
        ValueRef::Double(inner) => Value::Float(inner),
        // Rendered as a float rather than as exact digits, because the other systems in the matrix
        // return the same aggregate as a double and a comparison that only two of the three can
        // pass is not a comparison. CONFIG.md records this as the deviation it is.
        ValueRef::Decimal(inner) => Value::Float(decimal(inner)),
        ValueRef::Text(inner) => Value::Text(String::from_utf8_lossy(inner).into_owned()),
        ValueRef::Date32(inner) => Value::Date(inner),
        ValueRef::Time64(unit, inner) => Value::Time(microseconds(unit, inner, query, column)?),
        ValueRef::Timestamp(unit, inner) => {
            Value::Timestamp(microseconds(unit, inner, query, column)?)
        }
        other => {
            return Err(DriverError::unsupported(
                format!("{query} column {column}"),
                format!("DuckDB returned {other:?}, which this driver has no canonical form for"),
            ));
        }
    })
}

/// A wider integer as an `i64`, or a complaint.
///
/// No result in any workload here is larger than an `i64`, so one that is means the query is not
/// the one we think it is. Wrapping it silently would be the exact overflow this repository exists
/// to catch elsewhere, and `DuckDB` widening an aggregate to `HUGEINT` is precisely the case where a
/// system quietly gets it right and another one quietly does not.
fn narrow<T>(inner: T, query: &str, column: usize) -> Result<i64, DriverError>
where
    T: Copy + std::fmt::Display + TryInto<i64>,
{
    inner.try_into().map_err(|_| {
        DriverError::unsupported(
            format!("{query} column {column}"),
            format!("{inner} does not fit in a signed 64 bit integer"),
        )
    })
}

/// A decimal as a double.
///
/// The scaled payload divided by ten to the scale, which is what the decimal means. Both halves are
/// exact in a double for every width these workloads use, and the canonical form rounds to six
/// significant digits anyway.
fn decimal(inner: duckdb::types::Decimal) -> f64 {
    #[allow(
        clippy::cast_precision_loss,
        reason = "an i128 payload wider than a double's mantissa is already past what six \
                  significant digits would keep"
    )]
    let scaled = inner.value() as f64;
    scaled / 10_f64.powi(i32::from(inner.scale()))
}

/// A time or timestamp in microseconds, whatever unit `DuckDB` counted it in.
///
/// A nanosecond value that divides exactly is the same instant written in a coarser unit, so it is
/// converted. One with a remainder is refused rather than rounded, because rounding would make two
/// instants that differ render the same, and a canonical form that merges values is worse than one
/// that says it cannot render them.
fn microseconds(
    unit: duckdb::types::TimeUnit,
    inner: i64,
    query: &str,
    column: usize,
) -> Result<i64, DriverError> {
    use duckdb::types::TimeUnit;
    let factor = match unit {
        TimeUnit::Second => 1_000_000,
        TimeUnit::Millisecond => 1_000,
        TimeUnit::Microsecond => 1,
        TimeUnit::Nanosecond => {
            return if inner % 1_000 == 0 {
                Ok(inner / 1_000)
            } else {
                Err(DriverError::unsupported(
                    format!("{query} column {column}"),
                    format!("{inner} nanoseconds is finer than the canonical form goes"),
                ))
            };
        }
    };
    inner.checked_mul(factor).ok_or_else(|| {
        DriverError::unsupported(
            format!("{query} column {column}"),
            format!("{inner} {unit:?} does not fit in microseconds"),
        )
    })
}

#[cfg(test)]
mod tests {
    use bench_driver::Session;

    use super::*;

    fn setup(directory: &Path) -> Setup {
        Setup {
            directory: directory.to_path_buf(),
            threads: 2,
            memory: 1 << 30,
        }
    }

    #[test]
    fn it_reports_all_three_phases_against_a_real_database() {
        let scratch = tempfile::tempdir().unwrap();
        let table = scratch.path().join("numbers.csv");
        std::fs::write(&table, "1|one\n2|two\n3|three\n").unwrap();

        let mut driver = DuckDb::new();
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
                sql: "SELECT count(*), sum(column0) FROM numbers".to_owned(),
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
    fn it_asks_the_system_what_version_it_is() {
        let scratch = tempfile::tempdir().unwrap();
        let mut driver = DuckDb::new();

        // Before preparation there is nothing to ask, and answering from a constant in this file
        // would defeat the point of asking.
        assert_eq!(driver.version(), UNPREPARED);

        driver.prepare(&setup(scratch.path())).unwrap();
        let version = driver.version();
        assert!(
            version.starts_with('v'),
            "{version} does not look like a DuckDB version"
        );
        assert_ne!(version, UNPREPARED);
    }

    #[test]
    fn the_settings_it_was_given_are_the_ones_in_force() {
        let scratch = tempfile::tempdir().unwrap();
        let mut driver = DuckDb::new();
        driver.prepare(&setup(scratch.path())).unwrap();
        let connection = driver.connection().unwrap();

        let threads: i64 = connection
            .query_row("SELECT current_setting('threads')::BIGINT", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(threads, 2);

        let order: bool = connection
            .query_row(
                "SELECT current_setting('preserve_insertion_order')::BOOLEAN",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!order);

        let temp: String = connection
            .query_row("SELECT current_setting('temp_directory')", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(temp.starts_with(scratch.path().to_str().unwrap()));
    }

    #[test]
    fn every_type_it_can_return_has_a_canonical_form() {
        let scratch = tempfile::tempdir().unwrap();
        let mut driver = DuckDb::new();
        driver.prepare(&setup(scratch.path())).unwrap();

        let answer = driver
            .run(&Query {
                id: "types".to_owned(),
                sql: "SELECT true, 1::TINYINT, 2::BIGINT, 3::UBIGINT, 4.5::DOUBLE, \
                      6.25::DECIMAL(9, 2), 'text', NULL"
                    .to_owned(),
                ordered: true,
            })
            .unwrap();

        assert_eq!(answer.columns, 8);
        assert_eq!(answer.body, "true\t1\t2\t3\t4.5\t6.25\ttext\t\\N\n");
    }

    #[test]
    fn dates_and_timestamps_come_back_in_the_canonical_form_rather_than_duckdbs() {
        let scratch = tempfile::tempdir().unwrap();
        let mut driver = DuckDb::new();
        driver.prepare(&setup(scratch.path())).unwrap();

        let answer = driver
            .run(&Query {
                id: "when".to_owned(),
                sql: "SELECT DATE '2024-02-29', TIME '01:02:03.5', \
                      TIMESTAMP '1998-12-01 13:45:00', HUGEINT '170141183460469'"
                    .to_owned(),
                ordered: true,
            })
            .unwrap();

        assert_eq!(
            answer.body,
            "2024-02-29\t01:02:03.5\t1998-12-01 13:45:00\t170141183460469\n"
        );
    }

    #[test]
    fn an_integer_too_wide_for_the_canonical_form_is_refused_rather_than_wrapped() {
        // DuckDB widens an aggregate to HUGEINT where another system would overflow, and silently
        // wrapping it here would be the exact failure the digest comparison exists to catch.
        let scratch = tempfile::tempdir().unwrap();
        let mut driver = DuckDb::new();
        driver.prepare(&setup(scratch.path())).unwrap();

        let error = driver
            .run(&Query {
                id: "wide".to_owned(),
                sql: "SELECT HUGEINT '170141183460469231731687303715884105727'".to_owned(),
                ordered: true,
            })
            .unwrap_err();
        assert!(error.to_string().contains("64 bit"), "{error}");
    }

    #[test]
    fn a_type_with_no_canonical_form_is_refused_rather_than_stringified() {
        // A struct rendered as whatever DuckDB prints would look like an answer and digest
        // differently in every system, which is worse than saying so.
        let scratch = tempfile::tempdir().unwrap();
        let mut driver = DuckDb::new();
        driver.prepare(&setup(scratch.path())).unwrap();

        let error = driver
            .run(&Query {
                id: "struct".to_owned(),
                sql: "SELECT {'a': 1}".to_owned(),
                ordered: true,
            })
            .unwrap_err();
        assert!(matches!(error, DriverError::Unsupported { .. }), "{error}");
    }

    #[test]
    fn a_query_it_cannot_parse_says_which_query() {
        let scratch = tempfile::tempdir().unwrap();
        let mut driver = DuckDb::new();
        driver.prepare(&setup(scratch.path())).unwrap();

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
        assert_eq!(DuckDb::new().name(), "duckdb");
    }
}
