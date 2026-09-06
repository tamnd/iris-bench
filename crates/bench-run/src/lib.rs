//! The runner.
//!
//! Owns process isolation, randomised ordering with a recorded seed, the five storage tiers, and
//! cache control. Warm up is the runner's job and not the driver's, which is how one system does
//! not get warmed while another does not.
//!
//! Cache control and query ordering are the parts that exist so far. [`DropCaches`] is what
//! `bench_workload::protocol` asks before each query's three runs, and it is here rather than there
//! because dropping the page cache is a privileged operation on a machine and `bench-workload` is a
//! description of a protocol. [`Schedule`] is the order the queries are visited in, derived from a
//! [`Seed`] that goes into the record next to the numbers so the run can be replayed. The rest is
//! still to come. See the milestone that owns this crate in `docs/ROADMAP.md`.
//!
//! ```
//! use bench_run::{Schedule, Seed};
//!
//! let seed = Seed::from(0x0123_4567_89ab_cdef);
//! assert_eq!(seed.to_string(), "0123456789abcdef");
//!
//! // The seed is what a later run is handed, and it gives back the order the first run used.
//! let recorded = Schedule::new(seed, 0, 43);
//! let replayed = Schedule::new(seed.to_string().parse().unwrap(), 0, 43);
//! assert_eq!(recorded.order(), replayed.order());
//! ```

pub mod cache;
pub mod schedule;

pub use cache::DropCaches;
pub use schedule::{ParseSeedError, Schedule, Scheduled, Seed, run};

/// The version of this crate, as reported by build metadata.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
