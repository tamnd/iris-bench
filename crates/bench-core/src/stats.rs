//! Summarising a series of measurements.
//!
//! The headline statistic here is the median with a percentile bootstrap confidence interval. The
//! mean is not the headline because one descheduling event drags it and nothing drags it back. The
//! minimum is carried as its own column rather than folded into anything, because the minimum is a
//! statistic about the luckiest run and printing it where a reader expects a typical value is the
//! most common quiet distortion in this field.

use serde::{Deserialize, Serialize};

/// How a bootstrap interval is computed.
///
/// The seed is part of the configuration rather than taken from the clock, so summarising the same
/// samples twice gives the same interval. That matters more than it sounds: the adaptive stopping
/// rule is keyed on the width of this interval, and a rule built on a number that moves on its own
/// would stop at a different place every run.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Bootstrap {
    /// How many resamples to draw.
    ///
    /// Two thousand, not the ten thousand people reach for out of habit. Ten thousand is the right
    /// number for a percentile of a tail; for an interval on a median it moves the endpoints by
    /// less than the measurement noise they describe, and the adaptive stopping rule pays this cost
    /// repeatedly while a run is in progress.
    pub resamples: u32,
    /// The interval to report, as a fraction. 0.95 gives a 95% interval.
    pub confidence: f64,
    /// The seed for the resampling.
    pub seed: u64,
}

impl Default for Bootstrap {
    fn default() -> Self {
        Self {
            resamples: 2_000,
            confidence: 0.95,
            seed: 0x2545_f491_4f6c_dd1d,
        }
    }
}

/// What a series of measurements came to.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Summary {
    /// How many samples went into this.
    pub n: usize,
    /// The headline number.
    pub median: f64,
    /// Carried because it is what everyone else reports, not because it is the better statistic.
    pub mean: f64,
    /// The luckiest run. Its own column on purpose. See the note at the top of this file.
    pub min: f64,
    /// The unluckiest run. Useful mostly for spotting that something else was on the machine.
    pub max: f64,
    /// The low end of the confidence interval on the median.
    pub lo: f64,
    /// The high end of the confidence interval on the median.
    pub hi: f64,
    /// Which interval `lo` and `hi` describe, copied from the [`Bootstrap`] settings so a row is
    /// readable without them.
    pub confidence: f64,
}

impl Summary {
    /// The width of the interval as a fraction of the median.
    ///
    /// This is the number the adaptive stopping rule is keyed on. It is a property of one series
    /// and says nothing about any other series, which is the entire point.
    #[must_use]
    pub fn relative_width(&self) -> f64 {
        if self.median.abs() < f64::EPSILON {
            return f64::INFINITY;
        }
        (self.hi - self.lo) / self.median
    }
}

/// Summarises a series.
///
/// Returns `None` for an empty series, because there is no honest summary of nothing.
///
/// # Panics
///
/// Panics if a sample is not a number. A NaN in a timing series means the clock or the harness is
/// broken, and carrying on past that would put a made up number in a published table.
#[must_use]
pub fn summarise(samples: &[f64], boot: Bootstrap) -> Option<Summary> {
    if samples.is_empty() {
        return None;
    }
    assert!(
        samples.iter().all(|s| s.is_finite()),
        "a timing sample was not a finite number, which means the clock or the harness is broken"
    );

    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);

    // Sample counts here are in the hundreds. The cast is exact for anything under 2^53, which a
    // repetition count will never approach.
    #[allow(clippy::cast_precision_loss)]
    let mean = samples.iter().sum::<f64>() / samples.len() as f64;

    let (lo, hi) = bootstrap_median(&sorted, boot);

    Some(Summary {
        n: sorted.len(),
        median: median_of_sorted(&sorted),
        mean,
        min: sorted[0],
        max: sorted[sorted.len() - 1],
        lo,
        hi,
        confidence: boot.confidence,
    })
}

/// The coefficient of variation of a series: the standard deviation over the mean.
///
/// Unitless, which is the whole reason for it. One machine's spread in nanoseconds says nothing
/// next to another machine's, and the same spread as a fraction of the answer is directly
/// comparable, which is what the noise floor table in `docs/MACHINES.md` is made of.
///
/// Mean based rather than median based, because that is what the coefficient of variation is and
/// this number gets compared against figures published for other harnesses. Everywhere else here
/// the median is the headline, for the reason at the top of this file, and the difference is not an
/// inconsistency: a result wants the statistic that shrugs off one descheduling event, and a noise
/// floor wants the one that counts it.
///
/// `None` for fewer than two samples, because one sample has no spread, and for a mean at zero,
/// because dividing by that would report a machine as infinitely noisy when what actually happened
/// is that the clock did not move.
///
/// # Panics
///
/// Panics if a sample is not finite, for the same reason [`summarise`] does.
#[must_use]
pub fn coefficient_of_variation(samples: &[f64]) -> Option<f64> {
    if samples.len() < 2 {
        return None;
    }
    assert!(
        samples.iter().all(|s| s.is_finite()),
        "a timing sample was not a finite number, which means the clock or the harness is broken"
    );

    // Series lengths here are in the tens or hundreds.
    #[allow(clippy::cast_precision_loss)]
    let n = samples.len() as f64;

    let mean = samples.iter().sum::<f64>() / n;
    if mean.abs() < f64::EPSILON {
        return None;
    }

    // The n minus one divisor, because these are samples of how a machine behaves and not the whole
    // of it.
    let variance = samples.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / (n - 1.0);
    Some(variance.sqrt() / mean)
}

/// The median of an unsorted, non-empty slice, without sorting it.
///
/// The stopping rule asks for a summary while a run is in progress, so this runs thousands of times
/// per check. Selecting the middle element is linear where sorting is not, and the resample is
/// scratch space nobody looks at afterwards.
fn median_by_selection(draw: &mut [f64]) -> f64 {
    let n = draw.len();
    let (below, mid, _) = draw.select_nth_unstable_by(n / 2, f64::total_cmp);
    if n.is_multiple_of(2) {
        // Everything in `below` is at or under `mid`, so the largest of them is the other middle
        // element.
        let lower = below.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        f64::midpoint(lower, *mid)
    } else {
        *mid
    }
}

/// The median of an already sorted, non-empty slice.
fn median_of_sorted(sorted: &[f64]) -> f64 {
    let n = sorted.len();
    if n.is_multiple_of(2) {
        f64::midpoint(sorted[n / 2 - 1], sorted[n / 2])
    } else {
        sorted[n / 2]
    }
}

/// The percentile bootstrap. Resample with replacement, take the median of each resample, and read
/// the interval off the distribution of those medians.
fn bootstrap_median(sorted: &[f64], boot: Bootstrap) -> (f64, f64) {
    let n = sorted.len();
    if n == 1 || boot.resamples == 0 {
        // One sample says nothing about its own spread, and saying so with a zero width interval
        // would be a lie. The interval is the sample itself, and `n` in the row is what tells a
        // reader not to trust it.
        return (sorted[0], sorted[n - 1]);
    }

    let mut rng = SplitMix64::new(boot.seed);
    let mut medians = Vec::with_capacity(boot.resamples as usize);
    let mut draw = vec![0.0; n];
    for _ in 0..boot.resamples {
        for slot in &mut draw {
            *slot = sorted[rng.below(n)];
        }
        medians.push(median_by_selection(&mut draw));
    }
    medians.sort_by(f64::total_cmp);

    let tail = (1.0 - boot.confidence) / 2.0;
    (
        medians[quantile_index(medians.len(), tail)],
        medians[quantile_index(medians.len(), 1.0 - tail)],
    )
}

/// The index into a sorted slice of `len` items for a quantile in `0.0 ..= 1.0`.
fn quantile_index(len: usize, q: f64) -> usize {
    // Both the multiply and the cast are bounded by `len`, which is `resamples`.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let raw = (q * len as f64) as usize;
    raw.min(len - 1)
}

/// A small deterministic generator, so the crate does not take a dependency for this.
///
/// The bootstrap does not need a cryptographic generator or a long period. It needs the same
/// numbers every time, which is what makes a summary reproducible.
struct SplitMix64(u64);

impl SplitMix64 {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A value in `0 .. bound`, using the multiply and shift trick so there is no modulo bias worth
    /// caring about at these sizes.
    fn below(&mut self, bound: usize) -> usize {
        let r = u128::from(self.next());
        let b = bound as u128;
        // The high 64 bits of a 64 bit number times `bound` are always below `bound`, so this fits
        // in a usize whenever `bound` did.
        #[allow(clippy::cast_possible_truncation)]
        let out = ((r * b) >> 64) as usize;
        out
    }
}
