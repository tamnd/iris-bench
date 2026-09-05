//! The noise floor probe: how much a machine's answer moves when the question did not change.
//!
//! Every other number this harness produces is a difference between two measurements, and a
//! difference is only worth reporting when it is bigger than the spread of the thing it came from.
//! This measures that spread. It runs one fixed workload, restarts the process, runs it again, and
//! reports the coefficient of variation across those rounds.
//!
//! The result belongs in `docs/MACHINES.md` next to the class it was taken on, because it is a fact
//! about the machine rather than about anything being benchmarked, and because a reader looking at
//! a two percent effect needs to know what the floor under it was.
//!
//! # Why a round is a whole process
//!
//! Running the same workload a thousand times inside one process measures how steady the machine is
//! over the next few milliseconds, which is not the question. Address space layout, page placement
//! and allocator state are fixed for the life of a process and all three of them move a
//! measurement, so a probe that never restarts reports a floor far below the one a real comparison
//! stands on. A round here is therefore a process, which is [`bench_core::Level::Process`], and the
//! headline number is the spread across rounds.
//!
//! Rebuilding the binary between rounds is the level above that and this does not do it. That level
//! is real, it is what [`bench_core::Pilot`] exists to split out, and a rebuild takes longer than
//! everything else here put together. The floor reported by this probe is a floor under one build,
//! and a comparison across builds sits on a higher one.
//!
//! # The workload
//!
//! Four independent multiply and add chains over a four mebibyte buffer. It is not a decoder and it
//! is not trying to be one. What is wanted is a workload with no input, no allocation and no system
//! call inside the timed region, so that whatever moves between rounds is the machine and not the
//! work. Four mebibytes is larger than the level two cache on every machine in the fleet and
//! smaller than the level three cache on all of them, so the probe watches the core clock and the
//! path to the last level cache together, which is where a neighbouring tenant shows up first.

use std::process::Command;

use anyhow::{Context as _, bail};
use bench_core::{Bootstrap, Stop, Timing, coefficient_of_variation, summarise, time};
use bench_env::{Capture, Outcome};

/// How many 64 bit words the probe walks. Four mebibytes of them.
const WORDS: usize = 4 * 1024 * 1024 / 8;

/// How many chains run at once.
///
/// One chain measures the latency of a multiply and nothing else, because every step waits for the
/// one before it. Four of them keep the multiplier fed and let the loop run at the speed the bytes
/// arrive, which is the part worth watching for movement.
const CHAINS: usize = 4;

/// An odd multiplier with its bits spread out, so no chain settles into a short cycle.
const MULTIPLIER: u64 = 6_364_136_223_846_793_005;

/// One pass over the buffer, which is one sample.
///
/// The accumulators are folded together and returned so that nothing here is dead code to a
/// compiler that can see the whole function. [`bench_core::time`] puts the result through a black
/// box, and between the two of them the loop survives to be measured.
fn pass(buffer: &[u64]) -> u64 {
    let mut chains = [1u64; CHAINS];
    // The buffer is a whole number of steps long, so the remainder here is empty. It is dropped
    // rather than folded in, because a probe that walks a different number of words on a different
    // machine is measuring two things at once.
    let (steps, _) = buffer.as_chunks::<CHAINS>();
    for step in steps {
        for (chain, &word) in chains.iter_mut().zip(step) {
            *chain = chain.wrapping_mul(MULTIPLIER).wrapping_add(word);
        }
    }
    chains.iter().fold(0, |folded, chain| folded ^ chain)
}

/// The buffer, filled without a cast so the fill is the same arithmetic on every target.
fn buffer() -> Vec<u64> {
    let mut words = Vec::with_capacity(WORDS);
    let mut word = MULTIPLIER;
    for _ in 0..WORDS {
        word = word.wrapping_mul(MULTIPLIER).wrapping_add(1);
        words.push(word);
    }
    words
}

/// What one round came to.
#[derive(Clone, Copy, Debug)]
struct Round {
    /// The median sample in this round, in nanoseconds.
    median: f64,
    /// The spread within this round, as a fraction.
    within: f64,
}

/// Runs one round in this process and prints it for the parent to read.
///
/// Two numbers on one line, whitespace separated, in nanoseconds and then as a fraction. A format
/// with nothing to parse wrong is worth more here than a format that could be extended later, and
/// the parent and the child are the same binary so there is no version skew to design around.
///
/// # Errors
///
/// If the round produced no samples, or samples with no spread and no mean, which means the clock
/// did not move and there is nothing to report.
pub(crate) fn one_round(samples: u32, warmup: u32) -> anyhow::Result<()> {
    let round = measure(samples, warmup)?;
    println!("{} {}", round.median, round.within);
    Ok(())
}

/// Times the workload in this process.
fn measure(samples: u32, warmup: u32) -> anyhow::Result<Round> {
    let buffer = buffer();
    let timing = Timing {
        warmup,
        stop: Stop::After { samples },
        // `Stop::After` does not look at the series, so asking it after every sample costs a
        // comparison rather than a bootstrap.
        check_every: 1,
        bootstrap: Bootstrap::default(),
    };

    let series = timing.run(|| time(|| pass(&buffer)).1);
    let Some(summary) = series.summary() else {
        bail!("a round with no samples in it, which means the sample count was zero");
    };
    let Some(within) = coefficient_of_variation(series.samples()) else {
        bail!(
            "a round whose samples have no spread and no mean, which means the clock did not move"
        );
    };

    Ok(Round {
        median: summary.median,
        within,
    })
}

/// Runs the probe: `rounds` fresh processes, and the spread across what they answered.
///
/// The machine is read before the rounds and again after them, and a gate that failed at either end
/// stops this without printing a floor. A floor measured on a machine with something else running
/// on it is a measurement of the something else, and it is worse than no floor at all, because it
/// looks like a fact about the hardware and gets quoted as one. Reading the machine again at the
/// end is what catches the build that finished, the browser that woke up, and the backup that
/// started while the probe was running.
///
/// `anyway` measures regardless and says on every line of the report that it did. That exists
/// because what a busy machine looks like is a real question, and because somebody will want the
/// number for a machine that can never pass, which is better answered than worked around.
///
/// Being over the bar is not a failure and does not change the exit status. The virtualised class
/// is expected to be over it and finding out by how much is the reason to run this at all, so a non
/// zero exit there would turn the expected answer into an error nobody reads. `check` is the
/// command that refuses on the machine; this one refuses on the conditions and then measures.
///
/// # Errors
///
/// If a gate failed and `anyway` was not asked for, or if a round cannot be started, ends badly, or
/// prints something that is not two numbers.
pub(crate) fn probe(
    rounds: u32,
    samples: u32,
    warmup: u32,
    limit: f64,
    anyway: bool,
) -> anyhow::Result<()> {
    let capture = Capture::take();
    unfit(&capture, anyway, "before the probe started")?;

    let mut measured = Vec::with_capacity(rounds as usize);
    for _ in 0..rounds {
        measured.push(spawn_round(samples, warmup)?);
    }

    let after = Capture::take();
    unfit(&after, anyway, "by the time the probe finished")?;

    let medians: Vec<f64> = measured.iter().map(|round| round.median).collect();
    let withins: Vec<f64> = measured.iter().map(|round| round.within).collect();

    let Some(across) = coefficient_of_variation(&medians) else {
        bail!("{rounds} rounds is not enough to have a spread, try at least two");
    };
    let summary = summarise(&medians, Bootstrap::default()).expect("there is at least one round");
    let typical = summarise(&withins, Bootstrap::default()).expect("one per round");

    println!("{}", capture.class);
    println!("environment {}", capture.hash);
    println!(
        "{rounds} rounds of {samples} samples over {} MiB, {warmup} passes of warmup per round",
        WORDS * 8 / (1024 * 1024)
    );
    println!();
    println!("  round to round      {:>8.2}%", across * 100.0);
    println!(
        "  within one round    {:>8.2}%   median of the rounds",
        typical.median * 100.0
    );
    println!("  median round        {:>8.3} ms", summary.median / 1e6);
    println!("  fastest round       {:>8.3} ms", summary.min / 1e6);
    println!("  slowest round       {:>8.3} ms", summary.max / 1e6);
    println!();

    if across > limit {
        println!(
            "a round to round spread of {:.2}% is over the {:.2}% bar, so this class is ratios only",
            across * 100.0,
            limit * 100.0
        );
    } else {
        println!(
            "a round to round spread of {:.2}% is under the {:.2}% bar, so nothing this probe can \
             see rules this class out for durations",
            across * 100.0,
            limit * 100.0
        );
    }
    // Being quiet is necessary and not sufficient. The ceiling comes from what can be read about
    // the machine, and a machine nobody could check is capped whatever this probe measured.
    println!(
        "the class ceiling is unchanged by this: {}",
        capture.permits()
    );
    if anyway {
        println!(
            "this floor was taken with a gate failing, so it describes today on this machine and \
             not the machine"
        );
    }

    Ok(())
}

/// Stops the probe when the machine is not fit to be measured.
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
        "the {} gate failed {when}: {detail}. A floor measured on a machine with something else \
         running on it is a measurement of the something else. Wait for the machine to settle, or \
         ask for --anyway if what a busy machine looks like is the question",
        gate.name
    );
}

/// Starts one round in a new process and reads back what it measured.
fn spawn_round(samples: u32, warmup: u32) -> anyhow::Result<Round> {
    let binary = std::env::current_exe().context("finding this binary to start it again")?;
    let output = Command::new(&binary)
        .arg("noise")
        .arg("--one-round")
        .args(["--samples", &samples.to_string()])
        .args(["--warmup", &warmup.to_string()])
        .output()
        .with_context(|| format!("starting a round with {}", binary.display()))?;

    if !output.status.success() {
        bail!(
            "a round exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let text =
        String::from_utf8(output.stdout).context("a round printed something that is not text")?;
    let mut numbers = text.split_whitespace();
    let median = number(numbers.next(), &text)?;
    let within = number(numbers.next(), &text)?;
    Ok(Round { median, within })
}

/// One number out of what a round printed.
fn number(field: Option<&str>, whole: &str) -> anyhow::Result<f64> {
    let field =
        field.with_context(|| format!("a round printed {:?}, wanted two numbers", whole.trim()))?;
    field
        .parse()
        .with_context(|| format!("a round printed {field:?}, which is not a number"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_workload_does_the_same_thing_every_time() {
        // A probe whose answer moves between rounds is measuring itself. This is the check that the
        // work is fixed, so that a spread in the timings is a fact about the machine.
        let words = buffer();
        assert_eq!(pass(&words), pass(&words));
        assert_eq!(buffer(), words);
    }

    #[test]
    fn the_buffer_is_the_size_it_says_it_is() {
        assert_eq!(buffer().len() * 8, 4 * 1024 * 1024);
    }

    #[test]
    fn a_round_reports_a_median_and_a_spread() {
        // Two samples and no warmup, because what is being checked is that the round comes back
        // with numbers, and a real round takes long enough to be worth keeping out of the suite.
        let round = measure(2, 0).expect("two samples is a round");
        assert!(round.median > 0.0, "a pass took no measurable time");
        assert!(round.within >= 0.0);
    }
}
