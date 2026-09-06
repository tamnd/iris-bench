//! The trait every system under test implements, and what it gets told.

use std::path::PathBuf;

use crate::{Answer, Phase, WidthError};

/// A system under test.
///
/// Three methods, in the order they happen, and none of them times itself. See [`crate::Session`]
/// for why the clock lives outside.
///
/// Implementations go in `drivers/`, one crate per system, and each one carries a `CONFIG.md`
/// saying where its configuration came from. A driver that had to guess at a setting records that
/// it guessed, because a baseline configured by our own guesswork is the thing this whole
/// repository was built to stop publishing.
pub trait Driver {
    /// What the system is called, in the results. Lowercase, no version in it.
    ///
    /// Static, because a name is a constant of the system rather than something a driver works out
    /// from its own state, and a name that could vary between two calls is a name that could vary
    /// between two rows of the same table.
    fn name(&self) -> &'static str;

    /// What version of the system this is, as the system itself reports it.
    ///
    /// Read from the system rather than from a constant in the driver, so that a machine running
    /// something other than what the driver was written against says so.
    fn version(&self) -> String;

    /// Starts the system and applies its configuration.
    ///
    /// [`Setup`] carries no file paths, deliberately. See its documentation.
    ///
    /// # Errors
    ///
    /// If the system will not start or will not accept its configuration.
    fn prepare(&mut self, setup: &Setup) -> Result<(), DriverError>;

    /// Gets one table in, by whatever means this system gets tables in.
    ///
    /// Ingesting into a native format and registering a file where it lies are both correct answers
    /// here. They are different systems making a different trade, and the load timing is where that
    /// trade becomes visible.
    ///
    /// # Errors
    ///
    /// If the files cannot be read or the system will not take them.
    fn load(&mut self, load: &Load) -> Result<(), DriverError>;

    /// Answers one query.
    ///
    /// The [`Answer`] has to be materialised. A system with lazy evaluation that returns a plan
    /// here has been timed on building a plan, and the comparison against a system that actually
    /// computed the result is worthless.
    ///
    /// # Errors
    ///
    /// If the query fails, or if this system cannot express it. Use
    /// [`DriverError::unsupported`] for the second, so that the query is recorded as unsupported
    /// rather than as slow.
    fn run(&mut self, query: &Query) -> Result<Answer, DriverError>;
}

/// What a system is told before it sees any data.
///
/// **There are no file paths here, and that is the whole design.** A driver that was handed the
/// corpus at preparation time could read it, index it, convert it, or cache it, and all of that
/// would land in a phase that the load timing is supposed to account for. Preparation gets a
/// scratch directory, a thread count and a memory budget, and nothing that would let it start
/// early.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Setup {
    /// A directory the system may write to. Empty when preparation starts, and removed after.
    pub directory: PathBuf,
    /// How many threads the system is allowed to use.
    ///
    /// Set from the machine class rather than left to the system's own default, because a default
    /// that reads the host's core count makes every result a result about that host.
    pub threads: usize,
    /// How many bytes of memory the system is allowed to use.
    pub memory: u64,
}

/// One table to get in.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Load {
    /// What the queries call this table.
    pub table: String,
    /// The files it is in, already verified against the corpus manifest and already on this
    /// machine. A driver never fetches anything.
    pub files: Vec<PathBuf>,
    /// What is in those files.
    pub format: Format,
    /// What to select from the files, or `None` for every column as the files store it.
    ///
    /// A corpus does not always store a column as the type its queries ask about. `ClickBench`
    /// keeps `EventDate` as an unsigned sixteen bit count of days and three of its timestamps as
    /// plain Unix seconds, and every published entry converts those on the way in. That conversion
    /// is part of the benchmark's setup rather than something a driver should invent, so the
    /// workload supplies it and the driver applies it.
    ///
    /// It is a select list rather than a list of column names, because that is what the published
    /// setups are: `* REPLACE (make_date(EventDate) AS EventDate, ...)` for `DuckDB` and
    /// `* EXCEPT ("EventDate"), CAST(...) AS "EventDate"` for `DataFusion`. The two differ, and
    /// giving both systems one of them would be this repository configuring a benchmark rather
    /// than running the one its authors published.
    ///
    /// Where the cost of applying it lands is the driver's business and stays the driver's
    /// business. `DuckDB` publishes a load that materialises the conversion, `DataFusion` publishes
    /// one that expresses it as a view and pays it per query, and turning either into the other
    /// would hide the difference the three phases exist to show.
    pub projection: Option<String>,
}

/// What a corpus file is.
///
/// Small on purpose. Every corpus in scope is either Parquet or a text file with one row per line
/// and a single separator character, and a format enum with more in it than that would be
/// describing formats nothing here reads.
///
/// Not `non_exhaustive`, deliberately. Every driver lives in this workspace, and adding a format
/// should break each of their builds until someone has decided what that format means for that
/// system. The alternative is a wildcard arm in every driver, which is a decision nobody made
/// turning into a run that silently read the wrong thing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    /// Apache Parquet.
    Parquet,
    /// One row per line, fields separated by one character, with no header row. Covers the comma
    /// separated `ClickBench` text, the tab separated variants, and the pipe separated tables
    /// `dbgen` writes.
    Separated {
        /// The separator.
        separator: char,
    },
}

/// One query to answer.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Query {
    /// What the workload calls this query, such as `q23`. Goes in the result row.
    pub id: String,
    /// The SQL, already rewritten for this system if the workload's rules allow a rewrite. Any
    /// rewrite is recorded in the driver's `CONFIG.md` under deviations.
    pub sql: String,
    /// Whether the query specifies its own row order.
    ///
    /// When it does not, the rows are sorted before the result is digested, because two systems
    /// that returned the same rows in a different order both answered the question that was asked.
    /// See [`crate::answer`].
    pub ordered: bool,
}

/// What can go wrong in a driver.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DriverError {
    /// A phase was asked for before the phase it depends on.
    #[error("{phase} was asked for before {wanted}")]
    OutOfOrder {
        /// What was asked for.
        phase: Phase,
        /// What has to happen first.
        wanted: Phase,
    },

    /// A phase that happens once was asked for twice.
    #[error("{phase} happens once and was asked for again")]
    Repeated {
        /// Which phase.
        phase: Phase,
    },

    /// This system cannot express this query.
    ///
    /// Recorded as unsupported in the results rather than as a failure or, worse, as a fast time
    /// against an empty answer.
    #[error("{what} is not supported: {why}")]
    Unsupported {
        /// What could not be done, usually the query id.
        what: String,
        /// Why not, in enough detail that a reader can tell whether it is a limit of the system or
        /// a limit of the driver.
        why: String,
    },

    /// The rows a driver produced do not form a table.
    #[error("the result is not a table")]
    Answer(#[from] WidthError),

    /// Anything the system itself reported.
    ///
    /// Both halves are in the message rather than only the context. What the protocol records
    /// against a failed query is this error rendered as a string, so a message that stopped at the
    /// context would put `running q38` in the result and throw away the sentence saying which cast
    /// the system refused. A failure nobody can read is a failure nobody can fix, and seven of them
    /// in a row is how a driver that was never configured properly gets mistaken for a system that
    /// cannot answer.
    #[error("{context}: {source}")]
    System {
        /// What was being attempted.
        context: String,
        /// What the system said.
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl DriverError {
    /// Says that this system cannot express something.
    pub fn unsupported(what: impl Into<String>, why: impl Into<String>) -> Self {
        Self::Unsupported {
            what: what.into(),
            why: why.into(),
        }
    }

    /// Wraps whatever the system under test returned, with a note about what was being attempted.
    pub fn system(
        context: impl Into<String>,
        source: impl Into<Box<dyn std::error::Error + Send + Sync>>,
    ) -> Self {
        Self::System {
            context: context.into(),
            source: source.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preparation_cannot_be_handed_the_data() {
        // Written as a test rather than only as a comment, because the day someone adds a files
        // field to Setup is the day loading stops being measurable, and a compile error is a
        // better place to have that conversation than a review.
        let setup = Setup {
            directory: PathBuf::from("scratch"),
            threads: 4,
            memory: 1 << 30,
        };
        let rendered = format!("{setup:?}");
        assert!(!rendered.contains("files"));
        assert!(!rendered.contains("table"));
    }

    #[test]
    fn an_unsupported_query_says_which_and_why() {
        let error = DriverError::unsupported("q29", "no regexp_replace in this system");
        assert_eq!(
            error.to_string(),
            "q29 is not supported: no regexp_replace in this system"
        );
    }

    #[test]
    fn a_system_error_keeps_what_the_system_said() {
        let inner = std::io::Error::other("connection reset");
        let error = DriverError::system("loading hits", inner);
        // In the message and not only in the source. The protocol records the message, so anything
        // left out of it is left out of the result as well.
        assert_eq!(error.to_string(), "loading hits: connection reset");
        assert!(std::error::Error::source(&error).is_some());
    }

    #[test]
    fn a_separator_is_a_character_rather_than_a_named_format() {
        // The three text corpora in scope differ only in this character, and naming them Csv, Tsv
        // and Tbl would mean adding a variant the next time a corpus picks a fourth one.
        assert_ne!(
            Format::Separated { separator: ',' },
            Format::Separated { separator: '|' }
        );
    }
}
