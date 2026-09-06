//! A set of queries over one corpus, and where that set came from.

use bench_driver::{Format, Query};

/// A published set of queries, with the provenance of the text.
///
/// The queries are carried verbatim from wherever their authors publish them, and [`Source`] says
/// where that is and what the file hashed to on the day it was fetched. A workload whose SQL was
/// typed out from memory is a workload nobody can place next to the leaderboard it is named after,
/// and the difference between the two is invisible on the page.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Workload {
    /// What the workload is called, in the results. Lowercase.
    pub name: &'static str,
    /// The corpus directory this reads, as named in `corpora/`.
    pub corpus: &'static str,
    /// What the queries call the table.
    pub table: &'static str,
    /// What the corpus files are.
    pub format: Format,
    /// Where the text came from.
    pub source: Source,
    /// The queries, in the order the workload publishes them.
    pub queries: Vec<Query>,
}

/// Where a set of queries was fetched from, and what it hashed to when it was.
///
/// The digest is checked against the text this crate carries, by a test, so an edit to a file that
/// says verbatim at the top of it fails the build rather than passing quietly into a table.
/// Owned rather than borrowed, because a source travels into a stored result and a result read
/// back off disk has nothing static to borrow from.
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Source {
    /// The address the file was fetched from.
    pub url: String,
    /// The day it was fetched, as `YYYY-MM-DD`.
    pub fetched: String,
    /// The `BLAKE3` digest of the file as fetched, lower case hex.
    pub blake3: String,
}

/// Splits a published query file into one [`Query`] per line.
///
/// Every file this crate carries is one query per line with no comments and no blank lines, which
/// is the shape the upstream harnesses read them in. A line that is empty after trimming is
/// skipped rather than turned into a query, and the caller checks the count it got.
///
/// `ordered` comes from whether the query names an order for itself. A query that did not is
/// sorted before it is digested, because two systems that returned the same rows in a different
/// order both answered the question that was asked.
pub(crate) fn split(text: &str, prefix: &str) -> Vec<Query> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .enumerate()
        .map(|(number, sql)| Query {
            id: format!("{prefix}{number}"),
            sql: sql.to_owned(),
            ordered: sql.to_ascii_uppercase().contains("ORDER BY"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_that_names_its_own_order_is_not_sorted_again() {
        let queries = split("SELECT a FROM t\nSELECT a FROM t ORDER BY a\n", "q");
        assert_eq!(queries[0].id, "q0");
        assert!(!queries[0].ordered);
        assert_eq!(queries[1].id, "q1");
        assert!(queries[1].ordered);
    }

    #[test]
    fn a_lower_case_order_by_counts_as_one() {
        // The files carried here spell it in upper case throughout, and a set fetched next year
        // might not, so the check is on the words rather than on the spelling.
        assert!(split("select a from t order by a", "q")[0].ordered);
    }

    #[test]
    fn a_blank_line_does_not_become_a_query_and_does_not_shift_the_numbering() {
        let queries = split("SELECT 1\n\n   \nSELECT 2\n", "q");
        assert_eq!(queries.len(), 2);
        assert_eq!(queries[1].id, "q1");
        assert_eq!(queries[1].sql, "SELECT 2");
    }
}
