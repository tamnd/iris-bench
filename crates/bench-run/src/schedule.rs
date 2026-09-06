//! The order the queries are visited in, and the seed that reproduces it.
//!
//! A benchmark that always runs its queries in the order the file lists them is a benchmark where
//! every query after the first one starts on a machine the previous query left behind. The effect is
//! not small and it is not evenly distributed either, because the queries that follow an expensive
//! scan are the ones that benefit, and which queries those are is a property of the file rather than
//! of anything being measured.
//!
//! Shuffling the order fixes that, and shuffling it without writing down how breaks something else:
//! a result nobody can re-run is a result nobody can check. So the order comes from a seed, the seed
//! goes into the record next to the numbers, and the same seed gives back the same order.
//!
//! # What this does and does not remove
//!
//! It removes warming between queries. Query five no longer finds the columns query four touched
//! sitting in a buffer pool, a CPU cache, or a plan cache, because which query ran before it is now
//! different on every seed.
//!
//! It does not remove warming inside a query, and it must not. The protocol runs each query three
//! times back to back and reports the first as cold and the best of the rest as hot, and the second
//! and third runs being warmed by the first is the entire definition of the hot number. Randomising
//! within that would not be a fairer measurement, it would be a different one.
//!
//! It also does not, on its own, make a cold number cold. Dropping the page cache does that, and
//! [`crate::DropCaches`] is what the protocol asks. Shuffling and cache dropping answer two
//! different questions and a run wants both.

use std::fmt;

use bench_driver::Session;
use bench_workload::{PageCache, Report, Workload, measure};
use rand::SeedableRng as _;
use rand::seq::SliceRandom as _;
use rand_chacha::ChaCha20Rng;

/// What the run derives its schedule from.
///
/// Sixty four bits, printed as sixteen lower case hex characters, and the same rule the digests are
/// held to: one spelling, so that a seed copied out of a result and pasted into a command line is
/// the seed that was recorded rather than something that has to be argued about first.
#[derive(Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
// Through a string rather than a number, because a `u64` written as a JSON number is a `u64` read
// back as an `f64` by most of the things that will ever read one of these files, and a seed that
// loses its bottom bits in transit is a seed that no longer reproduces anything.
#[serde(into = "String", try_from = "String")]
pub struct Seed(u64);

impl Seed {
    /// Draws a seed nobody chose.
    ///
    /// For the first run of a session. Every later run of that session should pass the seed it was
    /// given back in, which is the whole point of recording it.
    #[must_use]
    pub fn fresh() -> Self {
        Self(rand::random())
    }

    /// The sixty four bits.
    #[must_use]
    pub fn get(self) -> u64 {
        self.0
    }
}

impl From<u64> for Seed {
    fn from(bits: u64) -> Self {
        Self(bits)
    }
}

impl fmt::Display for Seed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

impl fmt::Debug for Seed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Seed({self})")
    }
}

impl From<Seed> for String {
    fn from(seed: Seed) -> Self {
        seed.to_string()
    }
}

impl TryFrom<String> for Seed {
    type Error = ParseSeedError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.parse()
    }
}

impl std::str::FromStr for Seed {
    type Err = ParseSeedError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.len() != 16
            || !text
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        {
            return Err(ParseSeedError);
        }
        u64::from_str_radix(text, 16)
            .map(Self)
            .map_err(|_| ParseSeedError)
    }
}

/// A string that is not sixteen lower case hex characters.
#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
#[error("a seed is sixteen lower case hex characters")]
pub struct ParseSeedError;

/// One pass over the queries, in the order that pass visits them.
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Schedule {
    seed: Seed,
    pass: u32,
    order: Vec<usize>,
}

impl Schedule {
    /// Derives the order for one pass.
    ///
    /// The pass number is here so that running the workload more than once under one seed gives a
    /// different order each time. A session that recorded a list of seeds instead would be a session
    /// somebody has to keep the list in order to re-run, and one number is easier to carry through a
    /// results table than a list is.
    #[must_use]
    pub fn new(seed: Seed, pass: u32, len: usize) -> Self {
        let mut order: Vec<usize> = (0..len).collect();
        order.shuffle(&mut stream(seed, pass));
        Self { seed, pass, order }
    }

    /// The order the file lists them in, which is the one this module exists to avoid.
    ///
    /// Here so that a deliberate unshuffled run still records an order rather than recording
    /// nothing. A result with no schedule on it reads as a result nobody wrote the order down for,
    /// and that is a different and worse thing than a result whose order was the obvious one. The
    /// seed is zero because there is no seed, and no shuffle was drawn from it.
    #[must_use]
    pub fn identity(len: usize) -> Self {
        Self {
            seed: Seed(0),
            pass: 0,
            order: (0..len).collect(),
        }
    }

    /// The seed this order came from.
    #[must_use]
    pub fn seed(&self) -> Seed {
        self.seed
    }

    /// Which pass over the workload this is, counting from zero.
    #[must_use]
    pub fn pass(&self) -> u32 {
        self.pass
    }

    /// The positions, in the order they are visited.
    #[must_use]
    pub fn order(&self) -> &[usize] {
        &self.order
    }

    /// The items in the order this schedule visits them.
    ///
    /// # Panics
    ///
    /// If the slice is not the length the schedule was built for. Reordering forty three queries
    /// with a schedule built for forty two would silently drop one and report on the rest, and a
    /// missing query is exactly the kind of thing a geomean hides.
    #[must_use]
    pub fn apply<'a, T>(&self, items: &'a [T]) -> Vec<&'a T> {
        assert_eq!(
            items.len(),
            self.order.len(),
            "a schedule for {} items cannot order {}",
            self.order.len(),
            items.len()
        );
        self.order.iter().map(|&at| &items[at]).collect()
    }
}

/// Turns a seed and a pass number into the stream the shuffle draws from.
///
/// The thirty two bytes are hashed here rather than handed to whatever `rand` currently does with a
/// `u64`, because a schedule that a dependency upgrade can change is a schedule the recorded seed
/// does not actually pin. `ChaCha20` for the same reason: it is specified somewhere other than in
/// this repository and it will produce the same stream in five years. That covers the stream and not
/// the shuffle, which is still `rand`'s, and the test that asserts one known order is what covers
/// the rest.
fn stream(seed: Seed, pass: u32) -> ChaCha20Rng {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"iris-bench query schedule v1");
    hasher.update(&seed.get().to_le_bytes());
    hasher.update(&pass.to_le_bytes());
    ChaCha20Rng::from_seed(*hasher.finalize().as_bytes())
}

/// A report and the schedule that produced it.
///
/// The two travel together because they are only meaningful together. A set of query timings without
/// the order they were taken in is a set of timings nobody can reproduce, and an order without the
/// timings is nothing at all.
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Scheduled {
    /// The order the queries were visited in.
    pub schedule: Schedule,
    /// What each query did.
    pub report: Report,
}

/// Runs a workload in a shuffled order and keeps the schedule with the result.
///
/// The queries come back in the order they ran rather than in workload order, because that is the
/// order the numbers were taken in and re-sorting a report to look tidy loses the one thing the
/// shuffle was for.
#[must_use]
pub fn run(
    session: &mut Session<'_>,
    workload: &Workload,
    cache: &mut dyn PageCache,
    seed: Seed,
    pass: u32,
) -> Scheduled {
    let schedule = Schedule::new(seed, pass, workload.queries.len());
    let shuffled = Workload {
        queries: schedule
            .apply(&workload.queries)
            .into_iter()
            .cloned()
            .collect(),
        ..workload.clone()
    };
    let report = measure(session, &shuffled, cache);
    Scheduled { schedule, report }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_order_and_a_different_one_does_not() {
        let one = Schedule::new(Seed::from(7), 0, 43);
        let again = Schedule::new(Seed::from(7), 0, 43);
        let other = Schedule::new(Seed::from(8), 0, 43);
        assert_eq!(one.order(), again.order());
        assert_ne!(one.order(), other.order());
    }

    #[test]
    fn a_second_pass_under_one_seed_visits_them_in_a_different_order() {
        let first = Schedule::new(Seed::from(7), 0, 43);
        let second = Schedule::new(Seed::from(7), 1, 43);
        assert_ne!(first.order(), second.order());
        assert_eq!(first.seed(), second.seed());
        assert_eq!(second.pass(), 1);
    }

    #[test]
    fn an_unshuffled_run_still_records_an_order() {
        let plain = Schedule::identity(43);
        assert_eq!(plain.order(), (0..43).collect::<Vec<_>>().as_slice());
        assert_eq!(plain.seed(), Seed::from(0));
        // And it is not the order any real seed produced, which is what stops one being mistaken
        // for the other when the two sit next to each other in a results directory.
        assert_ne!(plain.order(), Schedule::new(Seed::from(0), 0, 43).order());
    }

    #[test]
    fn every_query_is_visited_exactly_once() {
        let mut visited = Schedule::new(Seed::fresh(), 0, 43).order().to_vec();
        assert_eq!(visited.len(), 43);
        visited.sort_unstable();
        assert_eq!(visited, (0..43).collect::<Vec<_>>());
    }

    #[test]
    fn the_order_is_not_the_order_the_file_lists_them_in() {
        // Forty three items have enough orders that a shuffle landing back on the original one is
        // not something to design around, but a broken shuffle returning the input is, and this is
        // the assertion that tells the two apart.
        let schedule = Schedule::new(Seed::from(1), 0, 43);
        assert_ne!(schedule.order(), (0..43).collect::<Vec<_>>().as_slice());
    }

    #[test]
    fn the_stream_is_pinned_rather_than_left_to_whatever_rand_does_this_year() {
        // The value is not interesting. Its being fixed is: if this fails after a dependency bump,
        // every seed recorded before the bump has stopped reproducing its run, and that is worth
        // finding out from a test rather than from a result nobody can replay.
        assert_eq!(
            Schedule::new(Seed::from(0), 0, 8).order(),
            &[6, 5, 1, 2, 3, 4, 0, 7]
        );
    }

    #[test]
    fn items_come_back_in_the_order_the_schedule_names() {
        let schedule = Schedule::new(Seed::from(3), 0, 5);
        let items = ["a", "b", "c", "d", "e"];
        let applied = schedule.apply(&items);
        let expected: Vec<&&str> = schedule.order().iter().map(|&at| &items[at]).collect();
        assert_eq!(applied, expected);
    }

    #[test]
    #[should_panic(expected = "a schedule for 5 items cannot order 4")]
    fn a_schedule_built_for_another_workload_is_refused_rather_than_truncating_it() {
        let _ = Schedule::new(Seed::from(3), 0, 5).apply(&["a", "b", "c", "d"]);
    }

    #[test]
    fn a_seed_reads_back_as_the_seed_it_printed() {
        let seed = Seed::from(0x0123_4567_89ab_cdef);
        assert_eq!(seed.to_string(), "0123456789abcdef");
        assert_eq!(seed.to_string().parse::<Seed>().unwrap(), seed);
        assert_eq!(Seed::from(1).to_string(), "0000000000000001");
    }

    #[test]
    fn an_upper_case_or_short_spelling_is_refused_rather_than_normalised() {
        assert!("0123456789ABCDEF".parse::<Seed>().is_err());
        assert!("1".parse::<Seed>().is_err());
        assert!("".parse::<Seed>().is_err());
        assert!("00000000000000zz".parse::<Seed>().is_err());
    }
}
