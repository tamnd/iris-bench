//! Where to spend the repetition budget.
//!
//! The contribution in Kalibera and Jones is not "run it more times". It is that the variance in a
//! measurement lives at a particular level, and repetitions are only worth anything at the level
//! that carries it. Running one process a thousand times when the variance is between processes
//! produces a very tight confidence interval around the wrong number, and the tightness of that
//! interval is what makes it convincing.
//!
//! So a pilot runs a small balanced design across all three levels, the variance is decomposed, and
//! the budget goes where the variance is.

use serde::{Deserialize, Serialize};

use crate::Error;

/// The three levels a repetition can happen at.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Level {
    /// Rebuild the binary. Link order and layout change, and Mytkowicz and colleagues showed that
    /// moves a measurement by more than most reported effects.
    Build,
    /// Start the process again. Address space layout, allocator state and page placement change.
    Process,
    /// Run the workload again inside the same process.
    Iteration,
}

impl core::fmt::Display for Level {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Build => "build",
            Self::Process => "process",
            Self::Iteration => "iteration",
        })
    }
}

/// The result of a pilot: a balanced set of samples across all three levels.
///
/// Balanced means every build ran the same number of processes and every process ran the same
/// number of iterations. An unbalanced design can still be decomposed, but the arithmetic stops
/// being something a reader can check by hand, and this crate would rather refuse than be the only
/// thing that knows whether the numbers are right.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Pilot {
    samples: Vec<Vec<Vec<f64>>>,
}

impl Pilot {
    /// Wraps a set of pilot samples indexed as `[build][process][iteration]`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Unbalanced`] if the design is ragged, or [`Error::PilotTooSmall`] if any
    /// level has fewer than two repetitions. Two is the minimum that says anything about a level:
    /// with one, that level's variance is not unmeasured, it is unmeasurable, and reporting it as
    /// zero would be a guess dressed as a result.
    pub fn new(samples: Vec<Vec<Vec<f64>>>) -> Result<Self, Error> {
        let builds = samples.len();
        let processes = samples.first().map_or(0, Vec::len);
        let iterations = samples.first().and_then(|b| b.first()).map_or(0, Vec::len);

        if builds < 2 || processes < 2 || iterations < 2 {
            return Err(Error::PilotTooSmall {
                builds,
                processes,
                iterations,
            });
        }
        if samples.iter().any(|b| b.len() != processes)
            || samples.iter().flatten().any(|p| p.len() != iterations)
        {
            return Err(Error::Unbalanced);
        }
        if samples.iter().flatten().flatten().any(|s| !s.is_finite()) {
            return Err(Error::NotFinite);
        }
        Ok(Self { samples })
    }

    /// How many builds the pilot ran.
    #[must_use]
    pub fn builds(&self) -> usize {
        self.samples.len()
    }

    /// How many processes per build.
    #[must_use]
    pub fn processes(&self) -> usize {
        self.samples[0].len()
    }

    /// How many iterations per process.
    #[must_use]
    pub fn iterations(&self) -> usize {
        self.samples[0][0].len()
    }

    /// Splits the variance across the three levels.
    ///
    /// This is the standard nested random effects decomposition. Each level's component is the
    /// mean square at that level minus the mean square at the level below, divided by how many
    /// measurements sit under one unit of it. A component that comes out negative is noise around
    /// zero and is reported as zero.
    #[must_use]
    pub fn decompose(&self) -> Components {
        // Pilot designs are a handful of repetitions at each level. Every cast below is of a count
        // in the low tens.
        #[allow(clippy::cast_precision_loss)]
        let (n1, n2, n3) = (
            self.builds() as f64,
            self.processes() as f64,
            self.iterations() as f64,
        );

        let all: Vec<f64> = self.samples.iter().flatten().flatten().copied().collect();
        #[allow(clippy::cast_precision_loss)]
        let grand = all.iter().sum::<f64>() / all.len() as f64;

        let build_means: Vec<f64> = self
            .samples
            .iter()
            .map(|b| mean(&b.iter().flatten().copied().collect::<Vec<_>>()))
            .collect();
        let process_means: Vec<Vec<f64>> = self
            .samples
            .iter()
            .map(|b| b.iter().map(|p| mean(p)).collect())
            .collect();

        // Written as loops rather than as iterator chains so that the three sums of squares sit
        // next to each other and can be checked against a textbook without unpicking anything.
        let mut ss_build = 0.0;
        let mut ss_process = 0.0;
        let mut ss_iteration = 0.0;
        for (i, build) in self.samples.iter().enumerate() {
            ss_build += (build_means[i] - grand).powi(2);
            for (j, process) in build.iter().enumerate() {
                ss_process += (process_means[i][j] - build_means[i]).powi(2);
                for sample in process {
                    ss_iteration += (sample - process_means[i][j]).powi(2);
                }
            }
        }
        ss_build *= n2 * n3;
        ss_process *= n3;

        let ms_build = ss_build / (n1 - 1.0);
        let ms_process = ss_process / (n1 * (n2 - 1.0));
        let ms_iteration = ss_iteration / (n1 * n2 * (n3 - 1.0));

        Components {
            build: ((ms_build - ms_process) / (n2 * n3)).max(0.0),
            process: ((ms_process - ms_iteration) / n3).max(0.0),
            iteration: ms_iteration.max(0.0),
        }
    }
}

fn mean(xs: &[f64]) -> f64 {
    // Called on pilot cells, which hold a handful of samples each.
    #[allow(clippy::cast_precision_loss)]
    let n = xs.len() as f64;
    xs.iter().sum::<f64>() / n
}

/// How much of the variance sits at each level.
///
/// The units are the square of whatever the samples were in, so these are only meaningful next to
/// each other. [`Components::share`] is usually the more useful view.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Components {
    /// Variance between builds of the same source.
    pub build: f64,
    /// Variance between runs of the same binary.
    pub process: f64,
    /// Variance between iterations inside one process.
    pub iteration: f64,
}

impl Components {
    /// The sum of the three.
    #[must_use]
    pub fn total(&self) -> f64 {
        self.build + self.process + self.iteration
    }

    /// What fraction of the variance sits at a level, in `0.0 ..= 1.0`.
    #[must_use]
    pub fn share(&self, level: Level) -> f64 {
        let total = self.total();
        if total <= 0.0 {
            return 0.0;
        }
        self.at(level) / total
    }

    /// The component at one level.
    #[must_use]
    pub fn at(&self, level: Level) -> f64 {
        match level {
            Level::Build => self.build,
            Level::Process => self.process,
            Level::Iteration => self.iteration,
        }
    }

    /// The level carrying the most variance.
    ///
    /// Ties go to the cheaper level, because when two levels look the same the honest reading is
    /// that the pilot could not tell them apart, and spending the budget on rebuilds in that case
    /// buys nothing.
    #[must_use]
    pub fn dominant(&self) -> Level {
        if self.build > self.process && self.build > self.iteration {
            Level::Build
        } else if self.process > self.iteration {
            Level::Process
        } else {
            Level::Iteration
        }
    }
}

/// How many repetitions to run at each level.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Plan {
    /// How many times to build.
    pub builds: u32,
    /// How many processes to start per build.
    pub processes: u32,
    /// How many iterations to run per process.
    pub iterations: u32,
}

impl Plan {
    /// How many measurements the plan produces in total.
    #[must_use]
    pub fn total(&self) -> u32 {
        self.builds
            .saturating_mul(self.processes)
            .saturating_mul(self.iterations)
    }
}

/// How many repetitions to keep at a level the pilot says carries almost no variance.
///
/// Two and not one. One gives no way to notice later that the level started carrying variance after
/// all, which happens the first time somebody changes a compiler flag. Going above two spends
/// budget on a level a pilot has already ruled out.
const SETTLED: u32 = 2;

/// Turns a budget and a dominant level into a plan.
///
/// The budget is a total number of measurements. The two levels that are not dominant get two
/// repetitions each and everything left goes to the level that carries the variance.
#[must_use]
pub fn plan(budget: u32, dominant: Level) -> Plan {
    let budget = budget.max(SETTLED * SETTLED * SETTLED);
    let rest = (budget / (SETTLED * SETTLED)).max(SETTLED);
    match dominant {
        Level::Iteration => Plan {
            builds: SETTLED,
            processes: SETTLED,
            iterations: rest,
        },
        Level::Process => Plan {
            builds: SETTLED,
            processes: rest,
            iterations: SETTLED,
        },
        Level::Build => Plan {
            builds: rest,
            processes: SETTLED,
            iterations: SETTLED,
        },
    }
}
