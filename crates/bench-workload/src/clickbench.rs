//! `ClickBench`, as its authors publish it.
//!
//! Forty three queries over one table of anonymised web analytics traffic. The corpus is pinned in
//! `corpora/clickbench-hits/manifest.toml` and the queries are the files below, carried byte for
//! byte from the `ClickBench` repository and checked against a digest by a test in this module.
//!
//! # Why there are three files and not one
//!
//! `ClickBench` does not publish one set of SQL. It publishes a directory per system, and each
//! directory has its own `queries.sql` rewritten for the dialect that system speaks. So each driver
//! gets the file published for it, and the `ClickHouse` reference is carried too, because it is the
//! text the leaderboard's own numbers come from and both rewrites here are rewrites of it.
//!
//! How far a rewrite goes is not obvious from the outside, so here is what the files fetched on the
//! date below actually say. The `DataFusion` set differs from the reference on forty two of the
//! forty three, almost all of it quoting every column name. The `DuckDB` set differs on two, and
//! both of those are `AVG(length(x))` written as `AVG(STRLEN(x))`. Two out of forty three looks
//! small enough to ignore and it is not: a query the system will not parse is not a slow query, it
//! is no query, and forty one right answers next to two errors is a geomean over forty one.
//!
//! It is also why the `DuckDB` set cannot simply be the reference. The gap is small, it is real,
//! and it is exactly the kind of gap somebody closes by hand in a hurry and then nobody can tell
//! whether the number came from `ClickBench`'s query or from ours.
//!
//! # What this module does not do
//!
//! It does not rewrite anything itself. If a system needs a rewrite that upstream has not
//! published, that is a deviation, and a deviation goes in that driver's `CONFIG.md` where a reader
//! will find it, not into a string in this file where they will not.

use bench_driver::Format;

use crate::workload::{Source, Workload, split};

/// How many queries `ClickBench` has. Asserted rather than counted, because a fetch that stopped
/// early still parses and a set of forty one queries would produce a geomean that looks fine.
pub const QUERIES: usize = 43;

/// The corpus directory these queries read.
pub const CORPUS: &str = "clickbench-hits";

/// What the queries call the table.
pub const TABLE: &str = "hits";

/// Which system's rewrite of the queries.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dialect {
    /// The `ClickHouse` set, which is the reference the other two are rewrites of.
    ClickHouse,
    /// The `DuckDB` set.
    DuckDb,
    /// The `DataFusion` set.
    DataFusion,
}

/// The day all three files below were fetched. One date, because they were fetched together and
/// three dates would invite them to drift apart.
const FETCHED: &str = "2026-09-06";

const CLICKHOUSE_SQL: &str = include_str!("../queries/clickhouse.sql");
const DUCKDB_SQL: &str = include_str!("../queries/duckdb.sql");
const DATAFUSION_SQL: &str = include_str!("../queries/datafusion.sql");

impl Dialect {
    /// The file, verbatim.
    #[must_use]
    pub fn sql(self) -> &'static str {
        match self {
            Self::ClickHouse => CLICKHOUSE_SQL,
            Self::DuckDb => DUCKDB_SQL,
            Self::DataFusion => DATAFUSION_SQL,
        }
    }

    /// Where the file came from and what it hashed to when it was fetched.
    #[must_use]
    pub fn source(self) -> Source {
        match self {
            Self::ClickHouse => Source {
                url: "https://raw.githubusercontent.com/ClickHouse/ClickBench/main/clickhouse/queries.sql"
                    .to_owned(),
                fetched: FETCHED.to_owned(),
                blake3: "175d7eecd195e7938865e7c321f2843590fe3ff9b5644c1185c8ec43c931ddd6".to_owned(),
            },
            Self::DuckDb => Source {
                url: "https://raw.githubusercontent.com/ClickHouse/ClickBench/main/duckdb/queries.sql"
                    .to_owned(),
                fetched: FETCHED.to_owned(),
                blake3: "0774b2d418dc4b36eb964ef6758771f649670bba2513acde81c2eb9070edd6e3".to_owned(),
            },
            Self::DataFusion => Source {
                url: "https://raw.githubusercontent.com/ClickHouse/ClickBench/main/datafusion/queries.sql"
                    .to_owned(),
                fetched: FETCHED.to_owned(),
                blake3: "709cced0f8d6a780a5e754dea059b516f24efdeaad23223ca63a4c895f3a78fc".to_owned(),
            },
        }
    }

    /// What this dialect is called in a result row.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::ClickHouse => "clickhouse",
            Self::DuckDb => "duckdb",
            Self::DataFusion => "datafusion",
        }
    }
}

/// Which published set of queries a driver gets, by the name the driver calls itself.
///
/// `duckdb` and `datafusion` get their own, which is the whole reason the three files are carried
/// separately.
///
/// `arrow-parquet` gets `DataFusion`'s. It has no `ClickBench` entry of its own and it is never
/// going to have one, because it is a reader rather than an engine and every one of the forty three
/// comes back unsupported from it. It still has to be given a set, so that the unsupported is
/// recorded against the same forty three queries as everything else rather than against nothing,
/// and `DataFusion`'s is the one it should get: `DataFusion` is the largest user of that reader and
/// the two are the same family of code. The choice is arbitrary in effect and it is written down
/// here rather than left to whichever set happened to be the default.
///
/// Anything else returns `None`. A driver added later picks its set deliberately, at this match,
/// and not by falling through to whatever the reference happens to be.
#[must_use]
pub fn dialect(driver: &str) -> Option<Dialect> {
    match driver {
        "clickhouse" => Some(Dialect::ClickHouse),
        "duckdb" => Some(Dialect::DuckDb),
        "datafusion" | "arrow-parquet" => Some(Dialect::DataFusion),
        _ => None,
    }
}

/// The workload, for one system's rewrite of the queries.
///
/// # Panics
///
/// If the file this crate carries does not hold exactly [`QUERIES`] queries. That is a build time
/// property of a file compiled into the binary, so a panic here means the file was edited, which is
/// the thing the digest test in this module exists to catch first.
#[must_use]
pub fn workload(dialect: Dialect) -> Workload {
    let queries = split(dialect.sql(), "q");
    assert_eq!(
        queries.len(),
        QUERIES,
        "the {} set carried here has {} queries and ClickBench has {QUERIES}",
        dialect.name(),
        queries.len(),
    );
    Workload {
        name: "clickbench",
        corpus: CORPUS,
        table: TABLE,
        // The single file Parquet distribution, which is what the manifest pins and what every
        // published entry in scope here reads. The text distribution exists and is a different
        // measurement.
        format: Format::Parquet,
        source: dialect.source(),
        queries,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Dialect; 3] = [Dialect::ClickHouse, Dialect::DuckDb, Dialect::DataFusion];

    #[test]
    fn the_files_are_the_ones_that_were_fetched() {
        // The one check that makes verbatim mean something. Everything else in this crate is about
        // what to do with the queries, and none of it is worth anything if the text drifted.
        // Every file is checked before anything is asserted, so a run that finds two drifted files
        // says so once rather than being fixed twice.
        let drifted: Vec<String> = ALL
            .into_iter()
            .filter_map(|dialect| {
                let digest = blake3::hash(dialect.sql().as_bytes()).to_hex().to_string();
                (digest != dialect.source().blake3).then(|| {
                    format!(
                        "the {} file hashes to {digest} and the source says {}",
                        dialect.name(),
                        dialect.source().blake3,
                    )
                })
            })
            .collect();
        assert!(drifted.is_empty(), "{}", drifted.join("\n"));
    }

    #[test]
    fn every_dialect_has_all_forty_three() {
        for dialect in ALL {
            assert_eq!(workload(dialect).queries.len(), QUERIES);
        }
    }

    #[test]
    fn the_rewrites_really_are_rewrites_and_not_copies() {
        // If they were copies this crate could carry one file and pass it to everything, and the
        // day that becomes true is a day somebody should have to look at rather than a day the code
        // quietly keeps doing the more expensive thing. The DuckDB gap is two queries, which is
        // small enough that this is the only place it is ever going to be noticed.
        let reference = workload(Dialect::ClickHouse).queries;
        for dialect in [Dialect::DuckDb, Dialect::DataFusion] {
            let rewritten = workload(dialect).queries;
            let differing = reference
                .iter()
                .zip(&rewritten)
                .filter(|(one, two)| one.sql != two.sql)
                .count();
            assert!(
                differing > 0,
                "the {} set is the reference set, so nothing here needs to carry both",
                dialect.name(),
            );
        }
    }

    #[test]
    fn the_duckdb_rewrite_is_the_two_queries_the_module_says_it_is() {
        // Named rather than counted, because "two of forty three" on the page above is the sort of
        // claim that stays there long after it stopped being true.
        let reference = workload(Dialect::ClickHouse).queries;
        let duckdb = workload(Dialect::DuckDb).queries;
        let differing: Vec<&str> = reference
            .iter()
            .zip(&duckdb)
            .filter(|(one, two)| one.sql != two.sql)
            .map(|(one, _)| one.id.as_str())
            .collect();

        assert_eq!(differing, ["q27", "q28"]);
        for id in differing {
            let query = duckdb.iter().find(|query| query.id == id).unwrap();
            assert!(query.sql.contains("STRLEN"));
        }
    }

    #[test]
    fn every_query_reads_the_table_the_corpus_is_loaded_as() {
        for dialect in ALL {
            for query in workload(dialect).queries {
                assert!(
                    query.sql.to_ascii_lowercase().contains("from hits"),
                    "{} {} does not read hits",
                    dialect.name(),
                    query.id,
                );
            }
        }
    }

    #[test]
    fn the_queries_are_numbered_the_way_the_leaderboard_numbers_them() {
        let queries = workload(Dialect::DuckDb).queries;
        assert_eq!(queries[0].id, "q0");
        assert_eq!(queries[QUERIES - 1].id, "q42");
        assert_eq!(queries[0].sql, "SELECT COUNT(*) FROM hits;");
    }

    #[test]
    fn most_of_the_queries_name_their_own_order_and_some_do_not() {
        // The ones that do not are the reason the canonical form sorts. Query 17 is the well known
        // case: a group by with a limit and no order by, so which ten rows come back is up to the
        // engine, and two engines disagreeing there is not two engines disagreeing about the data.
        let queries = workload(Dialect::ClickHouse).queries;
        let ordered = queries.iter().filter(|query| query.ordered).count();
        assert_eq!(ordered, 32);
        assert!(!queries[17].ordered);
        assert!(queries[17].sql.to_ascii_uppercase().contains("LIMIT"));
    }

    #[test]
    fn a_driver_with_no_published_entry_still_gets_a_set_and_it_is_written_down_which() {
        assert_eq!(dialect("duckdb"), Some(Dialect::DuckDb));
        assert_eq!(dialect("datafusion"), Some(Dialect::DataFusion));
        assert_eq!(dialect("arrow-parquet"), Some(Dialect::DataFusion));
        assert_eq!(dialect("null"), None);
    }
}
