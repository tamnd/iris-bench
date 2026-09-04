//! The timing loop and the rule that decides when to stop.
//!
//! There is one thing in this file that matters more than the rest of it. The stopping rule is
//! handed the series being measured and nothing else. It cannot see a rival, a baseline, or a
//! previous run, because the signature does not let it.
//!
//! That is deliberate. Adding repetitions until a comparison becomes significant is how a result
//! gets manufactured, and it does not feel like cheating while you are doing it: you are just
//! collecting more data, and more data is good. The defence against it is not discipline, it is not
//! having the number available at the point where the decision is made.

use std::hint::black_box;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::stats::{Bootstrap, Summary, summarise};

/// A series of measurements of one thing, in nanoseconds.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Series {
    samples: Vec<f64>,
    bootstrap: Bootstrap,
}

impl Series {
    /// An empty series that will be summarised with `bootstrap`.
    #[must_use]
    pub const fn new(bootstrap: Bootstrap) -> Self {
        Self {
            samples: Vec::new(),
            bootstrap,
        }
    }

    /// Adds a sample.
    pub fn push(&mut self, nanos: f64) {
        self.samples.push(nanos);
    }

    /// How many samples there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether there are no samples.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// The samples, in the order they were taken.
    #[must_use]
    pub fn samples(&self) -> &[f64] {
        &self.samples
    }

    /// The summary, or `None` if nothing has been measured.
    ///
    /// # Panics
    ///
    /// Panics if a sample is not finite. See [`summarise`].
    #[must_use]
    pub fn summary(&self) -> Option<Summary> {
        summarise(&self.samples, self.bootstrap)
    }
}

/// When to stop collecting.
///
/// Every variant is a question about one series. There is no variant that takes a second series,
/// and there is no way to add one from outside this crate.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
#[non_exhaustive]
pub enum Stop {
    /// Take exactly this many samples. Simple, and the right choice when the run has a time budget
    /// rather than a precision target.
    After {
        /// How many.
        samples: u32,
    },
    /// Keep going until the confidence interval on the median is narrow enough.
    ///
    /// This is the rule the methodology asks for. The target is a property of the series on its
    /// own, so it is reached at the same point whatever else is being measured that day.
    IntervalWidth {
        /// Stop once the interval width divided by the median is at or below this.
        target: f64,
        /// Never stop before this many samples, however narrow the interval looks. An interval
        /// computed from four samples is narrow because there is nothing in it to be wide.
        min: u32,
        /// Always stop at this many, so a workload whose variance never settles ends the run
        /// instead of ending the day.
        max: u32,
    },
}

impl Stop {
    /// Whether to stop, given everything collected so far and nothing else.
    ///
    /// # Panics
    ///
    /// Panics if a sample is not finite. See [`summarise`].
    #[must_use]
    pub fn should_stop(self, series: &Series) -> bool {
        match self {
            Self::After { samples } => series.len() as u64 >= u64::from(samples),
            Self::IntervalWidth { target, min, max } => {
                let n = series.len() as u64;
                if n < u64::from(min) {
                    return false;
                }
                if n >= u64::from(max) {
                    return true;
                }
                series
                    .summary()
                    .is_some_and(|s| s.relative_width() <= target)
            }
        }
    }
}

/// The timing loop.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Timing {
    /// How many times to run the workload before anything is recorded.
    ///
    /// The first run of anything pays for cold caches, lazy binding and a cold branch predictor.
    /// Those are real costs and they are worth measuring, but they are a different measurement and
    /// mixing them into this one just makes this one noisier.
    pub warmup: u32,
    /// When to stop.
    pub stop: Stop,
    /// How often to ask the stopping rule, in samples.
    ///
    /// Asking after every single sample is not wrong, it is just wasteful: a bootstrap over the
    /// whole series is not free and one more sample rarely moves the interval enough to change the
    /// answer. A run therefore ends at a multiple of this, which is worth knowing when reading a
    /// sample count back.
    pub check_every: u32,
    /// How the interval is computed.
    pub bootstrap: Bootstrap,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            warmup: 5,
            stop: Stop::IntervalWidth {
                target: 0.02,
                min: 20,
                max: 500,
            },
            check_every: 8,
            bootstrap: Bootstrap::default(),
        }
    }
}

impl Timing {
    /// Runs `body` until the stopping rule says to stop, and returns what was measured.
    ///
    /// `body` is called once per sample and returns the elapsed nanoseconds for that sample. Use
    /// [`time`] inside it, or return a number from somewhere else if the workload times itself.
    ///
    /// # Panics
    ///
    /// Panics if a sample is not finite. See [`summarise`].
    pub fn run<F>(&self, mut body: F) -> Series
    where
        F: FnMut() -> f64,
    {
        for _ in 0..self.warmup {
            black_box(body());
        }
        let every = self.check_every.max(1) as usize;
        let mut series = Series::new(self.bootstrap);
        loop {
            series.push(body());
            if series.len() % every == 0 && self.stop.should_stop(&series) {
                return series;
            }
        }
    }
}

/// Times one call and returns its result along with the elapsed nanoseconds.
///
/// The result comes back through `black_box` so that a workload whose output is thrown away does
/// not get optimised into nothing, which is the classic way to measure a very fast empty loop and
/// publish it as a very fast implementation.
pub fn time<T, F: FnOnce() -> T>(f: F) -> (T, f64) {
    let start = Instant::now();
    let out = black_box(f());
    // `as_secs_f64` rather than `as_nanos`, because the second one needs a cast from u128 that
    // loses precision on paper and buys nothing here.
    (out, start.elapsed().as_secs_f64() * 1e9)
}
