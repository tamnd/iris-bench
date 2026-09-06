//! All forty three `ClickBench` queries, against all three systems, on a table small enough to keep
//! in a test.
//!
//! The corpus this workload is really run on is fourteen gigabytes and lives on a fleet machine.
//! Nothing about the protocol needs it. What needs proving here is that the forty three queries as
//! published parse and execute against each engine, that every answer is digested, and that the
//! engines agree with each other about what those answers are. A hundred rows of the right shape
//! proves all of that, and it proves it on every push rather than on the days somebody has the
//! corpus in front of them.
//!
//! The table has the `ClickBench` schema, all hundred and five columns of it, read from
//! `tests/hits.sql` which is the create statement `ClickBench` publishes for `DuckDB`. Reading it
//! rather than typing a schema out here is deliberate: a fixture with ninety of the columns would
//! pass every query that happened to name one of the ninety.
//!
//! The values are not `ClickBench`'s. They are made up, and most queries come back with no rows or
//! with a handful, which is fine. This is a test that the machinery runs and agrees, not a test of
//! what the answers are, and the day the real corpus produces a disagreement it will be the same
//! comparison code that says so.
//!
//! The types are `ClickBench`'s, though, and that is a correction rather than a detail. The create
//! statement above is what the table looks like after `DuckDB` has loaded it, with `EventDate` as a
//! date and three columns as timestamps. The published Parquet file this workload is really run on
//! stores none of those four that way: `EventDate` is an unsigned sixteen bit count of days, and
//! `EventTime`, `ClientEventTime` and `LocalEventTime` are plain Unix seconds in a signed sixty
//! four bit integer with no logical type on them. Every published entry converts them on the way
//! in, which is why the create statement disagrees with the file it is loaded from.
//!
//! An earlier version of this fixture wrote the converted types, so all forty three queries passed
//! here while seven of them failed on the real corpus, and the ones that did not fail were quietly
//! answering a different question. So the fixture now stores the four columns the way the corpus
//! stores them and each system is given the projection its own published setup applies. That makes
//! this a miniature of the corpus rather than a miniature of what the corpus becomes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow_array::{
    ArrayRef, Int16Array, Int32Array, Int64Array, RecordBatch, StringArray, UInt16Array,
};
use arrow_schema::{DataType, Field, Schema, TimeUnit};
use bench_driver::{Driver, Format, Load, Session, Setup};
use bench_run::{Schedule, Scheduled, Seed};
use bench_workload::clickbench;
use bench_workload::{Outcome, RUNS, Report, Warm, Workload, compare};
use parquet::arrow::ArrowWriter;

/// The create statement `ClickBench` publishes for `DuckDB`, verbatim.
///
/// <https://raw.githubusercontent.com/ClickHouse/ClickBench/main/duckdb/create.sql>, fetched
/// 2026-09-06.
const CREATE: &str = include_str!("hits.sql");

/// How many distinct groups the fixture has.
///
/// Every column except `UserID` and `EventTime` is a function of the group a row is in, so a
/// `GROUP BY` anywhere in the workload lands on these groups and no others.
const GROUPS: usize = 3;

/// How many distinct `UserID` and group pairs there are. `UserID` runs from zero to the group's own
/// number, so group two has three of them and group zero has one.
const PAIRS: usize = GROUPS * (GROUPS + 1) / 2;

/// How many rows the fixture has. Each pair gets a different number of rows, one for the first, two
/// for the second and so on, which is what makes every count in the workload different from every
/// other count. See the note on [`column`].
const ROWS: usize = PAIRS * (PAIRS + 1) / 2;

/// How many columns `ClickBench` has. The corpus manifest asserts the same number against the real
/// file, so a fixture that lost columns fails here rather than in a run nobody watched.
const COLUMNS: usize = 105;

/// The day the fixture's rows happened, as days since the epoch. `ClickBench` queries filter on a
/// window in July 2013 and a fixture outside it would answer every one of them with nothing.
///
/// Unsigned sixteen bit, because that is what the corpus stores and what every published setup
/// converts on the way in.
const EVENT_DATE: u16 = 15_901;

/// Noon on the same day, in seconds since the epoch. Each row gets its own second, so that a query
/// ordering by `EventTime` has one answer rather than a set of them.
const EVENT_TIME: i64 = 1_373_889_600;

/// The four columns the corpus stores as something other than what the create statement declares.
///
/// `EventDate` first, then the three that hold raw seconds, which is the order the arms below read
/// them in.
const CONVERTED: [&str; 4] = [
    "EventDate",
    "EventTime",
    "ClientEventTime",
    "LocalEventTime",
];

#[test]
fn all_forty_three_run_on_every_system_and_the_engines_agree() {
    let scratch = tempfile::tempdir().expect("a scratch directory");
    let file = write_hits(scratch.path());

    let mut duckdb = driver_duckdb::DuckDb::new();
    let mut datafusion = driver_datafusion::DataFusion::new();
    let mut reader = driver_arrow_parquet::ArrowParquet::default();
    let reports = vec![
        measure(&mut duckdb, scratch.path(), &file),
        measure(&mut datafusion, scratch.path(), &file),
        measure(&mut reader, scratch.path(), &file),
    ];

    for report in &reports {
        assert_eq!(
            report.queries.len(),
            clickbench::QUERIES,
            "{} was asked something other than the whole workload",
            report.driver,
        );
        // A failure is not the same as an unsupported, and this is the assertion that keeps them
        // apart. If an engine cannot run a published query that is a finding about that engine or
        // about the driver, and it should be read rather than averaged into a geomean.
        let failed: Vec<String> = report
            .queries
            .iter()
            .filter_map(|query| match &query.outcome {
                Outcome::Failed { why } => Some(format!("{} {}: {why}", report.driver, query.id)),
                Outcome::Answered { .. } | Outcome::Unsupported { .. } => None,
            })
            .collect();
        assert!(failed.is_empty(), "{}", failed.join("\n"));
    }

    let engines = &reports[..2];
    for report in engines {
        assert_eq!(
            report.answered(),
            clickbench::QUERIES,
            "{} did not answer the whole workload",
            report.driver,
        );
        for query in &report.queries {
            let Outcome::Answered { runs } = &query.outcome else {
                unreachable!("every query was answered");
            };
            assert_eq!(
                runs.len(),
                RUNS,
                "{} {} was not run three times",
                report.driver,
                query.id
            );
            assert!(
                query.cold().is_some() && query.hot().is_some(),
                "{} {} has no cold and hot pair",
                report.driver,
                query.id,
            );
            // Nothing in this test is root, so no run of it is entitled to the word cold.
            assert!(!query.cache.is_cold());
        }
    }

    // The reference reader answers none of them, which is what a reader rather than an engine looks
    // like from here. Recorded as forty three unsupported rows rather than as forty three failures
    // or, worse, forty three very fast times.
    let reader = &reports[2];
    assert_eq!(reader.answered(), 0);
    assert!(
        reader
            .queries
            .iter()
            .all(|query| matches!(query.outcome, Outcome::Unsupported { .. }))
    );

    let comparison = compare(&reports);
    assert!(
        comparison.disagreed.is_empty(),
        "the engines disagreed about {:?}",
        comparison
            .disagreed
            .iter()
            .map(|one| one.query.as_str())
            .collect::<Vec<_>>(),
    );
    // Every query was answered by both engines and by neither reader, so all forty three are
    // compared and none is in the alone or unanswered lists.
    assert_eq!(comparison.agreed.len(), clickbench::QUERIES);
    assert!(comparison.alone.is_empty());
    assert!(comparison.unanswered.is_empty());
}

#[test]
fn a_query_the_engines_answer_differently_would_be_caught() {
    // The test above passing means the comparison found nothing. This one is here so that it is
    // also known to be capable of finding something, since a comparison that always agrees and a
    // comparison that never looks read the same way from the outside.
    let scratch = tempfile::tempdir().expect("a scratch directory");
    let file = write_hits(scratch.path());

    let mut duckdb = driver_duckdb::DuckDb::new();
    let mut datafusion = driver_datafusion::DataFusion::new();
    let honest = measure(&mut duckdb, scratch.path(), &file);
    let mut fibbing = measure(&mut datafusion, scratch.path(), &file);
    // One row fewer than it really returned, which is the smallest lie an engine could tell.
    if let Outcome::Answered { runs } = &mut fibbing.queries[0].outcome {
        for run in runs.iter_mut() {
            run.rows += 1;
            run.digest = bench_workload::Digest::of(&bench_driver::Answer {
                rows: run.rows,
                columns: 1,
                body: "0\n".to_owned(),
            });
        }
    }

    let comparison = compare(&[honest, fibbing]);
    assert_eq!(comparison.disagreed.len(), 1);
    assert_eq!(comparison.disagreed[0].query, "q0");
    assert!(!comparison.clean());
}

#[test]
fn without_the_published_setup_the_seven_date_queries_fail() {
    // The test above passing is only worth something if this fixture is capable of failing the way
    // the real corpus did. Seven of the published DataFusion queries compare EventDate to a date
    // literal, and against the types the corpus actually stores that is a number against a string.
    // Those seven, q36 to q42, are exactly the seven that failed on the 14 GB corpus before the
    // setup was carried. If this test ever stops finding them the fixture has drifted back to
    // storing the converted types and the one above has stopped proving anything.
    let scratch = tempfile::tempdir().expect("a scratch directory");
    let file = write_hits(scratch.path());
    let directory = scratch.path().join("raw");
    std::fs::create_dir_all(&directory).expect("a directory");

    let mut driver = driver_datafusion::DataFusion::new();
    let mut session = Session::new(&mut driver);
    session
        .prepare(&Setup {
            directory,
            threads: 2,
            memory: 1 << 30,
        })
        .expect("the system starts");
    session
        .load(&Load {
            table: clickbench::TABLE.to_owned(),
            files: vec![file],
            format: Format::Parquet,
            projection: None,
        })
        .expect("the system takes the table");

    let report = bench_workload::measure(
        &mut session,
        &clickbench::workload(clickbench::Dialect::DataFusion),
        &mut Warm,
    );
    let failed: Vec<&str> = report
        .queries
        .iter()
        .filter(|query| matches!(query.outcome, Outcome::Failed { .. }))
        .map(|query| query.id.as_str())
        .collect();
    assert_eq!(
        failed,
        ["q36", "q37", "q38", "q39", "q40", "q41", "q42"],
        "the fixture no longer stores EventDate the way the corpus does"
    );
}

#[test]
fn a_recorded_seed_replays_the_order_the_queries_ran_in() {
    let scratch = tempfile::tempdir().expect("a scratch directory");
    let file = write_hits(scratch.path());
    let seed = Seed::from(0xc0ff_ee00_1234_5678);

    let mut duckdb = driver_duckdb::DuckDb::new();
    let shuffled = with_session(&mut duckdb, scratch.path(), &file, |session, workload| {
        bench_run::run(session, workload, &mut Warm, seed, 0)
    });

    // The report is in the order the queries ran rather than in the order the file lists them, and
    // every query ran exactly once. A shuffle that dropped one and ran another twice would still
    // produce forty three rows, so the second half of that is worth asserting separately.
    let listed: Vec<String> = (0..clickbench::QUERIES)
        .map(|at| format!("q{at}"))
        .collect();
    let ran: Vec<&str> = shuffled
        .report
        .queries
        .iter()
        .map(|query| query.id.as_str())
        .collect();
    assert_eq!(ran.len(), clickbench::QUERIES);
    assert_ne!(ran, listed);
    let mut once = ran.clone();
    once.sort_unstable();
    once.dedup();
    assert_eq!(once.len(), clickbench::QUERIES);

    // The seed survives being written down and read back, and the order comes out of it again with
    // none of the rest of the run in hand. That is the whole claim: a result row carries what
    // somebody else needs to run the same schedule.
    let written = serde_json::to_string(&shuffled).expect("a scheduled run serialises");
    let read: Scheduled = serde_json::from_str(&written).expect("and reads back");
    assert_eq!(read.schedule.seed(), seed);
    assert_eq!(read.schedule.pass(), 0);

    let replayed = Schedule::new(read.schedule.seed(), read.schedule.pass(), listed.len());
    assert_eq!(replayed.order(), shuffled.schedule.order());
    let replayed_ids: Vec<&str> = replayed
        .apply(&listed)
        .into_iter()
        .map(String::as_str)
        .collect();
    assert_eq!(replayed_ids, ran);

    // Running them in a different order is a different measurement and it had better not be a
    // different answer. If it is, the shuffle is carrying state between queries that nothing should
    // be carrying.
    let mut again = driver_duckdb::DuckDb::new();
    let plain = measure(&mut again, scratch.path(), &file);
    for query in &plain.queries {
        let same = shuffled
            .report
            .queries
            .iter()
            .find(|other| other.id == query.id)
            .expect("the shuffled run has every query the plain one does");
        assert_eq!(
            same.digest(),
            query.digest(),
            "{} answered differently when it ran in another position",
            query.id,
        );
    }
}

/// Runs the whole workload against one system, from an unstarted driver.
fn measure(driver: &mut dyn Driver, scratch: &Path, file: &Path) -> Report {
    with_session(driver, scratch, file, |session, workload| {
        bench_workload::measure(session, workload, &mut Warm)
    })
}

/// Starts one system, gives it the table, and hands back a session and the set of queries that
/// system is measured with.
///
/// A closure rather than a returned session, because a session borrows the driver it drives and a
/// test that wants both has to keep them on the same stack frame.
fn with_session<T>(
    driver: &mut dyn Driver,
    scratch: &Path,
    file: &Path,
    take: impl FnOnce(&mut Session<'_>, &Workload) -> T,
) -> T {
    let name = driver.name();
    let dialect = clickbench::dialect(name).expect("every driver here has a set");
    let workload = clickbench::workload(dialect);
    let directory = scratch.join(name);
    std::fs::create_dir_all(&directory).expect("a directory per system");

    let mut session = Session::new(driver);
    session
        .prepare(&Setup {
            directory,
            threads: 2,
            memory: 1 << 30,
        })
        .expect("the system starts");
    session
        .load(&Load {
            table: clickbench::TABLE.to_owned(),
            files: vec![file.to_owned()],
            format: Format::Parquet,
            // The system's own published setup, which is the point of the fixture storing the raw
            // types. The two engines convert different columns at different times and the reference
            // reader gets none, and all three of those are decided in one place in bench-workload.
            projection: clickbench::projection(name).map(str::to_owned),
        })
        .expect("the system takes the table");

    take(&mut session, &workload)
}

/// Writes the fixture and returns where it went.
fn write_hits(directory: &Path) -> PathBuf {
    let schema = Arc::new(schema());
    assert_eq!(schema.fields().len(), COLUMNS);

    let columns: Vec<ArrayRef> = schema.fields().iter().map(|field| column(field)).collect();
    let batch = RecordBatch::try_new(Arc::clone(&schema), columns).expect("the columns line up");

    let path = directory.join("hits.parquet");
    let file = std::fs::File::create(&path).expect("a file to write");
    let mut writer = ArrowWriter::try_new(file, schema, None).expect("a writer");
    writer.write(&batch).expect("the batch goes in");
    writer.close().expect("the footer goes on");
    path
}

/// The `ClickBench` schema, read out of the create statement rather than typed out again.
fn schema() -> Schema {
    let fields = CREATE
        .lines()
        .map(str::trim)
        .filter(|line| line.ends_with(',') || line.ends_with("NOT NULL"))
        .map(|line| {
            let line = line.trim_end_matches(',');
            let (name, rest) = line.split_once(' ').expect("a name and a type");
            let optional = !rest.ends_with("NOT NULL");
            let kind = rest.trim_end_matches("NOT NULL").trim();
            Field::new(name, stored(name, kind), optional)
        })
        .collect::<Vec<_>>();
    Schema::new(fields)
}

/// What the corpus stores a column as, which is not always what the create statement declares.
///
/// The create statement is the table after a load. Four of its columns are converted on the way in
/// by whichever setup the system publishes, and a fixture that skipped the conversion would be
/// testing the queries against a file nobody has.
fn stored(name: &str, kind: &str) -> DataType {
    match name {
        "EventDate" => DataType::UInt16,
        "EventTime" | "ClientEventTime" | "LocalEventTime" => DataType::Int64,
        _ => arrow(kind),
    }
}

/// What one of the create statement's types is in Arrow.
///
/// Panics on anything it has not seen, rather than falling back to text. A column that quietly
/// became a string is a column every comparison on it would then be doing lexically.
fn arrow(kind: &str) -> DataType {
    match kind.to_ascii_uppercase().as_str() {
        "BIGINT" => DataType::Int64,
        "INTEGER" => DataType::Int32,
        "SMALLINT" => DataType::Int16,
        "DATE" => DataType::Date32,
        "TIMESTAMP" => DataType::Timestamp(TimeUnit::Microsecond, None),
        text if text.starts_with("TEXT")
            || text.starts_with("VARCHAR")
            || text.starts_with("CHAR") =>
        {
            DataType::Utf8
        }
        other => panic!("the create statement has a {other} and this fixture does not know one"),
    }
}

/// One column of made up values.
///
/// # What the shape is for
///
/// Almost every `ClickBench` query is a group by with an order by and a limit on the end, and two
/// engines only have to give the same answer to one of those if the ordering key is different for
/// every row it returns. Ties are where a benchmark's result checking goes wrong: two engines that
/// returned the same ten groups in a different order have both answered correctly, and a digest
/// says they disagreed.
///
/// So the fixture has no ties anywhere the workload looks. Every column except `UserID` and
/// `EventTime` is a function of the row's group, `UserID` is a function of the group and the row's
/// place within it, and the number of rows behind every combination is different from the number
/// behind every other one. That makes `COUNT(*)`, `COUNT(DISTINCT UserID)` and
/// `AVG(length(URL))` all take a different value in every group, and it keeps the number of groups
/// under the smallest limit the workload uses, so a limit never has to choose between equals
/// either.
///
/// # What it is hiding
///
/// The real corpus has ties, in the same queries, and no arrangement of a fixture makes that go
/// away. What to do about them is the business of the milestone that compares digests on the real
/// data, and it is written down here so that this test passing is not read as that problem being
/// solved. The two known shapes are a group by with a limit and no order by at all, and an order by
/// on a count where the tenth and eleventh group have the same one.
fn column(field: &Field) -> ArrayRef {
    let shape = shape();
    match field.data_type() {
        DataType::Int16 => Arc::new(
            shape
                .iter()
                .map(|(group, _)| i16::try_from(*group).unwrap())
                .collect::<Int16Array>(),
        ),
        DataType::Int32 => Arc::new(
            shape
                .iter()
                .map(|(group, _)| i32::try_from(*group).unwrap())
                .collect::<Int32Array>(),
        ),
        // The count of days the corpus stores, rather than a date. Every published setup turns this
        // one into a date before a query sees it.
        DataType::UInt16 => Arc::new(UInt16Array::from(vec![EVENT_DATE; ROWS])),
        // Raw Unix seconds, which is what the corpus holds and what DuckDB's setup converts and
        // DataFusion's leaves for its queries to convert. A second per row, so that a query
        // ordering by EventTime has one answer, and all of them inside one minute, so that the
        // query grouping by the minute still sees one group.
        DataType::Int64 if CONVERTED.contains(&field.name().as_str()) => {
            Arc::new(Int64Array::from_iter_values(
                (0..ROWS).map(|row| EVENT_TIME + i64::try_from(row).unwrap()),
            ))
        }
        // UserID is the one integer that varies inside a group, because four of the queries count
        // the distinct ones per group and a column that was constant per group would give every
        // group the answer one.
        DataType::Int64 if field.name() == "UserID" => Arc::new(
            shape
                .iter()
                .map(|(_, user)| i64::try_from(*user).unwrap())
                .collect::<Int64Array>(),
        ),
        DataType::Int64 => Arc::new(
            shape
                .iter()
                .map(|(group, _)| i64::try_from(*group).unwrap())
                .collect::<Int64Array>(),
        ),
        DataType::Utf8 => Arc::new(StringArray::from_iter_values(
            shape.iter().map(|(group, _)| text(field.name(), *group)),
        )),
        other => panic!("the fixture has a {other} column and does not know what to put in it"),
    }
}

/// The group and the `UserID` of every row.
///
/// Group `g` holds the users zero to `g`, and each of those combinations gets one more row than the
/// one before it. Six combinations, one to six rows each, twenty one rows.
fn shape() -> Vec<(usize, usize)> {
    let mut rows = Vec::with_capacity(ROWS);
    let mut repeats = 1;
    for group in 0..GROUPS {
        for user in 0..=group {
            rows.extend(std::iter::repeat_n((group, user), repeats));
            repeats += 1;
        }
    }
    assert_eq!(rows.len(), ROWS);
    rows
}

/// A value for one text column of one group.
///
/// The lengths are all different, because the workload orders by `AVG(length(URL))` in two places
/// and equal lengths there are the same tie the counts were shaped to avoid.
fn text(name: &str, group: usize) -> String {
    let tail = "p".repeat(group + 1);
    match name {
        // The two the workload runs a regular expression over, so they have to look like addresses.
        // The pattern wants a scheme, an optional www, a host and a path, and a string that does not
        // match comes back whole, which is a different answer rather than a wrong one.
        "URL" | "Referer" => format!("http://www.example{group}.org/{tail}"),
        _ => format!("{name}-{tail}"),
    }
}
