//! What the harness adds to a measurement of something that does nothing.
//!
//! Every duration this repository publishes is the workload plus whatever the timing loop costs to
//! wrap around it. That cost does not cancel out of a ratio when the two sides run different
//! numbers of samples, it does not shrink when the machine is idle, and it is the one source of
//! error here that is a property of the code rather than of the hardware. So it gets measured
//! rather than assumed, and the number goes next to the noise floor in `docs/MACHINES.md`.
//!
//! # What overhead means when the answer depends on the workload
//!
//! There is no single percentage. The harness adds a roughly fixed number of nanoseconds per
//! sample, so the share it takes is large for a short workload and negligible for a long one, and
//! quoting one figure without the duration it was taken at says nothing. What this reports instead
//! is the fixed cost and the duration at which it falls under the bar, which is the form a reader
//! can apply to a workload that has not been written yet.
//!
//! # How the fixed cost is separated from the work
//!
//! Both sides of a pair run the same number of iterations of the same workload. One side reads the
//! clock once around the whole batch, the other reads it once per iteration. Dividing each by the
//! iteration count gives the cost of one iteration measured two ways, and the difference between
//! them is every clock read but one. Nothing else about the two sides differs, which is what makes
//! the subtraction mean something.
//!
//! This is the reason the comparison is not against a workload of nothing at all. A pair of clock
//! reads around an empty body does measure the clock, and it is reported below because it is worth
//! knowing, but it does not measure what the clock reads cost when there is real work between them
//! competing for the same out of order window.
//!
//! # The workload
//!
//! A dependent chain of multiply and add in registers, with the iteration count set by calibration
//! to hit each target duration. No memory, no allocation and no system call, so the sweep varies
//! duration and nothing else. The noise probe deliberately walks a buffer because it is watching
//! for a neighbouring tenant on the path to memory. This is watching for the clock, and a workload
//! that touches memory would put cache behaviour into a number that is supposed to be about two
//! calls to [`std::time::Instant::now`].

use anyhow::bail;
use bench_core::{Bootstrap, Ratio, paired_ratio, summarise, time};
use bench_env::{Capture, Outcome};
use std::hint::black_box;

/// An odd multiplier with its bits spread out, so the chain does not settle into a short cycle.
const MULTIPLIER: u64 = 6_364_136_223_846_793_005;

/// The durations the sweep visits, in nanoseconds.
///
/// Four decades, because the crossing point is expected to sit in the middle of them and a sweep
/// that only brackets it from one side cannot show that it was found rather than assumed.
const TARGETS: [f64; 5] = [100.0, 1_000.0, 10_000.0, 100_000.0, 1_000_000.0];

/// One duration's worth of results.
struct Row {
    /// How many chain steps this row ran per iteration.
    rounds: u64,
    /// One iteration, timed once around the whole batch.
    batched: f64,
    /// One iteration, timed on its own.
    separate: f64,
    /// What the harness adds to a reported sample, in nanoseconds.
    bias: f64,
    /// The second divided by the first.
    ratio: Ratio,
}

/// A dependent chain of multiply and add, `rounds` steps long.
///
/// Each step needs the result of the one before it, so the loop runs at the latency of a multiply
/// rather than at its throughput. That is what makes the duration proportional to `rounds` and
/// predictable enough to calibrate against.
fn spin(rounds: u64) -> u64 {
    let mut value = MULTIPLIER;
    for _ in 0..rounds {
        value = value.wrapping_mul(MULTIPLIER).wrapping_add(1);
    }
    value
}

/// Runs the chain with both the argument and the result hidden from the optimiser.
///
/// Hiding the result stops the call being deleted. Hiding the argument stops it being computed once
/// and reused, which is the failure that turns a batch of a hundred iterations into one iteration
/// and reports the harness as free.
fn opaque(rounds: u64) -> u64 {
    black_box(spin(black_box(rounds)))
}

/// How many chain steps take about `target` nanoseconds on this machine.
fn calibrate(target: f64) -> u64 {
    const PROBE: u32 = 1_000_000;
    // Three goes and the shortest wins. Calibration runs before the measurement and a single
    // descheduled probe would set every row's iteration count from one bad reading.
    let per_round = (0..3)
        .map(|_| time(|| opaque(u64::from(PROBE))).1 / f64::from(PROBE))
        .fold(f64::INFINITY, f64::min);
    if per_round <= 0.0 {
        return 1;
    }
    // The step count is a workload size rather than a measurement, so losing the fractional part
    // is the intent, and the clamp is what makes the conversion defined rather than the cast.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let steps = (target / per_round).round().clamp(1.0, 1e15) as u64;
    steps
}

/// Runs the gate.
///
/// # Errors
///
/// If the machine is not fit to measure and `anyway` was not asked for, or if the shortest workload
/// the harness can time within `bar` is longer than `floor`.
pub(crate) fn gate(
    pairs: u32,
    batch: u32,
    warmup: u32,
    bar: f64,
    floor: f64,
    anyway: bool,
) -> anyhow::Result<()> {
    if batch < 2 {
        bail!(
            "a batch of {batch} leaves no clock reads to count, since the difference between the \
             two sides is every read but one"
        );
    }

    let capture = Capture::take();
    unfit(&capture, anyway, "before the measurement started")?;

    let empty = empty_clock_pair(pairs, warmup);
    let rows = TARGETS
        .into_iter()
        .map(|target| measure(target, pairs, batch, warmup))
        .collect::<anyhow::Result<Vec<_>>>()?;

    let after = Capture::take();
    unfit(&after, anyway, "by the time the measurement finished")?;

    // The bias is read off the shortest row. It is the same number of nanoseconds in every row, so
    // every row is an estimate of it, and the shortest is the one where it is the largest share of
    // what was measured and therefore the one where the machine's own noise buries it last. The
    // other rows are printed so that a reader can see whether it really is constant, which is the
    // assumption the whole table rests on.
    let shortest = rows.first().expect("the sweep is not empty");
    let crossing = shortest.bias / bar;

    report(&capture, &rows, empty, batch, crossing, bar, floor);

    if anyway {
        println!();
        println!(
            "this was measured with a gate failing, so it describes today on this machine and not \
             the machine"
        );
    }

    if shortest.ratio.lo <= 1.0 {
        // A bias that cannot be shown to be positive is not a bias that has been measured, and
        // reporting a crossing point derived from it would put a number on noise. This fails rather
        // than passing, for the same reason `Ratio::within` asks the interval and not the midpoint.
        bail!(
            "the shortest row puts the instrumented side at {:.2}% of the clean one, interval \
             {:.2}% to {:.2}%, which includes parity, so this run cannot say what the harness adds. \
             That is a statement about the machine rather than about the timing loop, and the noise \
             floor in docs/MACHINES.md says whether this machine was ever going to resolve it",
            shortest.ratio.ratio * 100.0,
            shortest.ratio.lo * 100.0,
            shortest.ratio.hi * 100.0
        )
    } else if crossing <= floor {
        Ok(())
    } else {
        bail!(
            "the harness adds {:.1} ns to every sample it reports, so it is under {:.2}% only for \
             workloads longer than {}, and the shortest workload this fleet intends to time is {}. \
             Either the timing loop reads the clock more than it needs to or this machine's clock \
             is slow, and the {:.1} ns a pair of reads costs in wall clock says which",
            shortest.bias,
            bar * 100.0,
            duration(crossing),
            duration(floor),
            empty
        )
    }
}

/// What a pair of clock reads around an empty body costs, as a median in nanoseconds.
fn empty_clock_pair(pairs: u32, warmup: u32) -> f64 {
    for _ in 0..warmup {
        black_box(time(|| ()).1);
    }
    let samples: Vec<f64> = (0..pairs.max(1)).map(|_| time(|| ()).1).collect();
    summarise(&samples, Bootstrap::default()).map_or(0.0, |summary| summary.median)
}

/// Measures one target duration both ways.
fn measure(target: f64, pairs: u32, batch: u32, warmup: u32) -> anyhow::Result<Row> {
    let rounds = calibrate(target);
    let count = f64::from(batch);

    let mut batched_side = Vec::with_capacity(pairs as usize);
    let mut separate_side = Vec::with_capacity(pairs as usize);

    for pair in 0..pairs + warmup {
        // Both sides run `batch` iterations of the same chain. The only difference is how often the
        // clock is read, so the order is alternated for the same reason it is anywhere else: so
        // that whichever side goes first does not keep the advantage of going first.
        let (batched, separate) = if pair.is_multiple_of(2) {
            let batched = batched_pass(rounds, batch);
            let separate = separate_pass(rounds, batch);
            (batched, separate)
        } else {
            let separate = separate_pass(rounds, batch);
            let batched = batched_pass(rounds, batch);
            (batched, separate)
        };
        if pair >= warmup {
            batched_side.push(batched / count);
            separate_side.push(separate / count);
        }
    }

    // Separately timed over batched, so the number is how much the instrumented side costs
    // relative to the clean one and a reader does not have to invert it in their head.
    let Some(ratio) = paired_ratio(&separate_side, &batched_side, Bootstrap::default()) else {
        bail!("{pairs} pairs is not enough to compare, try at least one");
    };
    let batched = summarise(&batched_side, Bootstrap::default()).expect("at least one pair");
    let separate = summarise(&separate_side, Bootstrap::default()).expect("at least one pair");

    // Neither side is clean, and the difference between them is not the whole bias. Writing `b` for
    // the bias and `w` for one iteration of the workload, the separately timed side reports `w + b`
    // per iteration and the batched side reports `w + b/k`, because its one clock pair is divided
    // across `k` iterations. So the difference is `b` short by a factor of `k` and the correction
    // is exact rather than an estimate.
    let bias = (separate.median - batched.median) * count / (count - 1.0);

    Ok(Row {
        rounds,
        batched: batched.median,
        separate: separate.median,
        bias,
        ratio,
    })
}

/// `batch` iterations with the clock read once around all of them.
fn batched_pass(rounds: u64, batch: u32) -> f64 {
    time(|| {
        for _ in 0..batch {
            opaque(rounds);
        }
    })
    .1
}

/// `batch` iterations with the clock read once around each of them.
///
/// Only the timed regions are added up. The loop that adds them is outside every one of them, so
/// what comes back is what the harness would have recorded rather than how long the loop took.
fn separate_pass(rounds: u64, batch: u32) -> f64 {
    let mut total = 0.0;
    for _ in 0..batch {
        total += time(|| opaque(rounds)).1;
    }
    total
}

/// Prints what was measured.
fn report(
    capture: &Capture,
    rows: &[Row],
    empty: f64,
    batch: u32,
    crossing: f64,
    bar: f64,
    floor: f64,
) {
    println!("{}", capture.class);
    println!("environment {}", capture.hash);
    println!(
        "a pair of clock reads around an empty body takes {empty:.1} ns of wall clock, which is a \
         different measurement from the bias below and does not have to agree with it, because an \
         empty body lets one pair of reads overlap the next and a workload between them does not"
    );
    println!(
        "each side of a pair ran {batch} iterations, so the two sides differ by {} clock pairs and \
         the bias column is that difference scaled back to one",
        batch - 1
    );
    println!();
    println!("  workload      steps   instrumented         bias   overhead   interval");
    for row in rows {
        println!(
            "  {:>9}  {:>9}      {:>9}   {:>7.1} ns  {:>7.2}%   {:.2}% to {:.2}%",
            duration(row.batched),
            row.rounds,
            duration(row.separate),
            row.bias,
            (row.ratio.ratio - 1.0) * 100.0,
            (row.ratio.lo - 1.0) * 100.0,
            (row.ratio.hi - 1.0) * 100.0
        );
    }
    println!();
    println!(
        "the shortest row says the harness adds {:.1} ns to every sample it reports, so it is \
         under {:.2}% of any workload longer than {}",
        rows.first().map_or(0.0, |row| row.bias),
        bar * 100.0,
        duration(crossing)
    );
    if crossing <= floor {
        println!(
            "the shortest workload this fleet intends to time is {}, so the harness is under the \
             bar for everything it will be asked to time",
            duration(floor)
        );
    } else {
        println!(
            "the shortest workload this fleet intends to time is {}, which is shorter than that, \
             so the timing loop decides the answer for the workloads at that end",
            duration(floor)
        );
    }
    println!(
        "this machine is good for {}, and these are ratios taken inside one run",
        capture.permits()
    );
}

/// Formats a duration in nanoseconds with a unit a reader does not have to count zeroes in.
fn duration(nanos: f64) -> String {
    if nanos < 1_000.0 {
        format!("{nanos:.0} ns")
    } else if nanos < 1_000_000.0 {
        format!("{:.1} us", nanos / 1_000.0)
    } else {
        format!("{:.2} ms", nanos / 1_000_000.0)
    }
}

/// Stops the measurement when the machine is not fit to measure.
fn unfit(capture: &Capture, anyway: bool, when: &str) -> anyhow::Result<()> {
    let Some(gate) = capture.failed() else {
        return Ok(());
    };
    if anyway {
        return Ok(());
    }
    let detail = match &gate.outcome {
        Outcome::Pass { observed } => observed.clone(),
        Outcome::Fail { observed, wanted } => format!("{observed}, wanted {wanted}"),
    };
    bail!(
        "the {} gate failed {when}: {detail}. This measures tens of nanoseconds, which is the scale \
         a single descheduling event dwarfs, so a reading taken next to something else running \
         would be that something else. Wait for the machine to settle, or ask for --anyway if a \
         working note is what is wanted",
        gate.name
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shortest of three runs of the chain, in nanoseconds.
    ///
    /// The same defence [`calibrate`] uses, for the same reason. These tests run alongside every
    /// other test in the workspace, so a single reading here is a reading of whatever else the
    /// machine was doing, and descheduling can only ever make a duration longer.
    fn shortest(rounds: u64) -> f64 {
        (0..3)
            .map(|_| time(|| opaque(rounds)).1)
            .fold(f64::INFINITY, f64::min)
    }

    #[test]
    fn a_longer_chain_takes_longer() {
        // Calibration is arithmetic on the assumption that duration is proportional to the step
        // count. If that stops being true, every row in the sweep aims at the wrong duration and
        // nothing else in this file notices.
        let short = shortest(1_000);
        let long = shortest(1_000_000);
        assert!(long > short * 10.0, "short {short}, long {long}");
    }

    #[test]
    fn calibration_lands_within_an_order_of_magnitude_of_what_it_aimed_at() {
        let rounds = calibrate(100_000.0);
        let taken = shortest(rounds);
        assert!(
            (10_000.0..1_000_000.0).contains(&taken),
            "aimed at 100000 ns with {rounds} steps and took {taken}"
        );
    }

    #[test]
    fn a_batch_of_one_is_refused_because_it_would_compare_a_thing_against_itself() {
        let error = gate(1, 1, 0, 0.01, 1e6, true).unwrap_err();
        assert!(format!("{error}").contains("no clock reads to count"));
    }

    #[test]
    fn the_two_sides_run_the_same_amount_of_work() {
        // Not a timing assertion. It checks that neither pass has been edited into running a
        // different number of iterations from the other, which would show up as overhead.
        let batched = batched_pass(10_000, 4);
        let separate = separate_pass(10_000, 4);
        assert!(batched > 0.0 && separate > 0.0);
        assert!(
            separate < batched * 10.0,
            "batched {batched}, separate {separate}"
        );
    }
}
