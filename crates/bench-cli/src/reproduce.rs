//! `iris-bench reproduce`: run a registered claim and say which of the five words it came to.
//!
//! The claim is read from the registry in `bench-store`, which is committed files rather than
//! anything this command constructs. So the threshold in front of a reader is the one that was
//! written down before the number existed, and the ordering is checkable rather than asserted.
//! Nothing here can widen a bar, and [`bench_store::claim`] refuses to grade a reading taken before
//! its own registration.
//!
//! # This is not a gate
//!
//! `resident` and `overhead` exit non zero when they miss their bar, because they are gates and a
//! claim that stops holding should stop a build. This exits zero on every verdict including
//! `NOT-REPRODUCED`, because a failed reproduction is a result and the third of the three rules in
//! the README is that losses are published. What it exits non zero on is being unable to produce a
//! verdict at all: an entry that cannot be graded, a claim nobody registered, or an instrument this
//! repository has not written yet.
//!
//! That last one is deliberately an error rather than `NOT-ATTEMPTABLE`. Work nobody has started is
//! a gap in this repository, and giving it a verdict would put a finished looking row on a page for
//! something that was never attempted.
//!
//! # The control comes first
//!
//! An instrument that cannot put two copies of the same thing at parity cannot report that two
//! different things are three percent apart. So a claim measured by a comparison runs its control
//! in the same session, on the same machine, minutes apart, and what the control misses parity by
//! is carried into the grading as the resolution. A reading that close to its bar is not decided in
//! either direction, which is the honest answer and is the one this would rather give than widen
//! the bar until the instrument clears it.

use anyhow::Context as _;
use bench_store::claim::{Attempt, Caveat, Claim, Instrument, Measured, Resident, Verdict};

use crate::resident::{self, Measurement, Setup};

/// Runs one registered claim and prints the verdict.
///
/// # Errors
///
/// If nothing is registered under `target`, if the instrument cannot be run, or if what came back
/// cannot be graded. Not on the verdict, whichever of the five it is.
pub(crate) fn run(
    target: &str,
    anyway: bool,
    out: Option<std::path::PathBuf>,
) -> anyhow::Result<()> {
    let claim = bench_store::claim::registered(target).ok_or_else(|| {
        let registered: Vec<String> = bench_store::claim::registry()
            .into_iter()
            .map(|claim| format!("{} {}", claim.id, claim.title))
            .collect();
        anyhow::anyhow!(
            "nothing is registered as {target}. What is registered:\n  {}",
            registered.join("\n  ")
        )
    })?;

    println!("{}, {}", claim.id, claim.title);
    println!(
        "registered {} in {}",
        claim.registered.day, claim.registered.source
    );
    println!(
        "the bar is {}, within {:.2}% of {:.3}",
        claim.bar.reading,
        claim.bar.tolerance * 100.0,
        claim.bar.target
    );
    if !claim.citable() {
        println!("registered as exploratory, so whatever this comes to cannot be cited as a claim");
    }
    println!();

    let attempt = attempt(&claim, anyway)?;
    let verdict = claim.decide(Some(&attempt))?;

    println!();
    println!("{verdict}");
    println!("{}", why(&claim, &attempt, verdict));
    if let Attempt::Measured(measured) = &attempt {
        for caveat in &measured.caveats {
            println!("  caveat: {}", caveat.what());
        }
    }

    if let Some(path) = out {
        let record = Record {
            claim: &claim,
            attempt: &attempt,
            verdict,
        };
        std::fs::write(&path, serde_json::to_string_pretty(&record)?)
            .with_context(|| format!("writing the verdict to {}", path.display()))?;
        println!("written to {}", path.display());
    }

    Ok(())
}

/// A verdict and everything it was drawn from, for the file `--out` writes.
///
/// The claim goes in alongside the reading rather than being referred to by identifier, so that the
/// file says what the threshold was on the day rather than what the registry says it is now.
#[derive(Debug, serde::Serialize)]
struct Record<'a> {
    claim: &'a Claim,
    attempt: &'a Attempt,
    verdict: Verdict,
}

/// Runs whatever instrument the claim registered.
fn attempt(claim: &Claim, anyway: bool) -> anyhow::Result<Attempt> {
    match claim.instrument {
        Instrument::Resident(resident) => windowed(resident, anyway),
    }
}

/// Runs the resident comparison, and its control, and turns the pair into one reading.
fn windowed(registered: Resident, anyway: bool) -> anyhow::Result<Attempt> {
    let setup = |control| -> anyhow::Result<Setup> {
        Ok(Setup {
            size: registered.size,
            span: usize::try_from(registered.span)
                .context("a window span that does not fit in memory")?,
            chunk: usize::try_from(registered.chunk)
                .context("a range that does not fit in memory")?,
            pairs: registered.pairs,
            control,
        })
    };

    // The control first. Running it after the reading would mean grading the reading against a
    // resolution measured on a machine that had since changed, and the whole reason the two are in
    // one session is that they are meant to be the same machine.
    let control = resident::measure(setup(true)?, registered.warmup, anyway)?;
    println!("{}", control.capture.class);
    println!("environment {}", control.capture.hash);
    line("control", &control);

    let reading = resident::measure(setup(false)?, registered.warmup, anyway)?;
    line("reading", &reading);

    let mut caveats = Vec::new();
    if control.capture.failed().is_some() || reading.capture.failed().is_some() {
        caveats.push(Caveat::GatesOverridden);
    }

    Ok(Attempt::Measured(Measured {
        day: today(),
        value: reading.ratio.ratio,
        resolution: resolution(&control),
        caveats,
    }))
}

/// What the control says this instrument can resolve, as a fraction.
///
/// The far end of the control's interval rather than its midpoint, because the bias is itself
/// measured with uncertainty and taking the near end would credit the instrument with a precision
/// the run did not establish.
fn resolution(control: &Measurement) -> f64 {
    let ratio = &control.ratio;
    (ratio.lo - 1.0).abs().max((ratio.hi - 1.0).abs())
}

/// One line of a run, as a percentage with its interval.
fn line(what: &str, measurement: &Measurement) {
    let ratio = &measurement.ratio;
    println!(
        "  {what:<8}  {:>8.2}%   interval {:.2}% to {:.2}%",
        ratio.ratio * 100.0,
        ratio.lo * 100.0,
        ratio.hi * 100.0
    );
}

/// Says in one sentence why the verdict is the word it is.
///
/// The arithmetic is printed rather than only the answer, because a reader who disagrees with the
/// verdict should be able to see which of the three numbers they disagree with.
fn why(claim: &Claim, attempt: &Attempt, verdict: Verdict) -> String {
    let Attempt::Measured(measured) = attempt else {
        return match attempt {
            Attempt::Refused(refused) => format!("{}, see {}", refused.what, refused.citation),
            Attempt::Measured(_) => unreachable!("matched above"),
        };
    };

    let off = (measured.value - claim.bar.target).abs() / claim.bar.target.abs() * 100.0;
    let bar = claim.bar.tolerance * 100.0;
    let resolution = measured.resolution * 100.0;

    match verdict {
        Verdict::Reproduced | Verdict::ReproducedWithCaveat => format!(
            "the reading is {off:.2}% off the target, the bar is {bar:.2}%, and the control puts \
             {resolution:.2}% beyond this instrument, so it is inside with room the instrument can \
             see"
        ),
        Verdict::NotReproduced => format!(
            "the reading is {off:.2}% off the target, the bar is {bar:.2}%, and the control puts \
             {resolution:.2}% beyond this instrument, so it is outside by more than the instrument \
             could be wrong by"
        ),
        Verdict::NotAttemptable => format!(
            "the reading is {off:.2}% off the target and the bar is {bar:.2}%, and the control puts \
             {resolution:.2}% beyond this instrument, which is more than the gap between them. This \
             harness cannot settle it either way, and widening the bar to a number it does clear \
             would be answering a different question"
        ),
        // Nothing this command produces is pending, because it has just run the thing.
        Verdict::Pending => "nothing was run".to_owned(),
    }
}

/// Today, as `YYYY-MM-DD` in the machine's own time zone.
fn today() -> String {
    jiff::Zoned::now().date().to_string()
}

#[cfg(test)]
mod tests {
    use bench_store::claim::{Bar, Purpose, Registration};

    use super::*;

    fn claim(target: f64, tolerance: f64) -> Claim {
        Claim {
            id: "C0002".to_owned(),
            title: "a title".to_owned(),
            purpose: Purpose::Confirmatory,
            registered: Registration {
                day: "2026-09-04".to_owned(),
                source: "an issue".to_owned(),
            },
            bar: Bar {
                reading: "one thing over another".to_owned(),
                target,
                tolerance,
            },
            instrument: Instrument::Resident(Resident {
                size: 1024,
                span: 1024,
                chunk: 512,
                pairs: 2,
                warmup: 0,
            }),
        }
    }

    fn measured(value: f64, resolution: f64) -> Attempt {
        Attempt::Measured(Measured {
            day: "2026-09-06".to_owned(),
            value,
            resolution,
            caveats: Vec::new(),
        })
    }

    #[test]
    fn the_sentence_under_a_verdict_carries_the_three_numbers_it_came_from() {
        let claim = claim(1.0, 0.03);
        let attempt = measured(1.4351, 0.0935);
        let verdict = claim.decide(Some(&attempt)).unwrap();
        let sentence = why(&claim, &attempt, verdict);

        assert_eq!(verdict, Verdict::NotReproduced);
        assert!(sentence.contains("43.51%"), "{sentence}");
        assert!(sentence.contains("3.00%"), "{sentence}");
        assert!(sentence.contains("9.35%"), "{sentence}");
    }

    #[test]
    fn a_reading_inside_what_the_control_cannot_see_says_so_rather_than_picking_a_side() {
        let claim = claim(1.0, 0.03);
        let attempt = measured(0.9672, 0.0935);
        let verdict = claim.decide(Some(&attempt)).unwrap();

        assert_eq!(verdict, Verdict::NotAttemptable);
        assert!(
            why(&claim, &attempt, verdict).contains("widening the bar"),
            "the reason a reading is not decided is worth saying out loud"
        );
    }

    #[test]
    fn the_resolution_is_the_far_end_of_the_control_interval() {
        // A control reported as 91.12% with an interval of 90.65% to 91.84% is 9.35% from parity at
        // its worst, not 8.88%. Grading against the midpoint would credit the instrument with a
        // precision this run did not establish.
        let ratio = bench_core::Ratio {
            ratio: 0.9112,
            lo: 0.9065,
            hi: 0.9184,
            confidence: 0.95,
            n: 60,
        };
        assert!((far(&ratio) - 0.0935).abs() < 1e-9, "{}", far(&ratio));
    }

    /// The arithmetic of [`resolution`], without a whole [`Measurement`] to carry it.
    fn far(ratio: &bench_core::Ratio) -> f64 {
        (ratio.lo - 1.0).abs().max((ratio.hi - 1.0).abs())
    }

    #[test]
    fn a_claim_nobody_registered_is_an_error_and_not_a_verdict() {
        let error = run("C9999", false, None).unwrap_err();
        let message = format!("{error}");
        assert!(
            message.contains("nothing is registered as C9999"),
            "{message}"
        );
        // And it says what is, because the next thing anybody does after this is guess again.
        assert!(message.contains("C0002"), "{message}");
    }

    #[test]
    fn today_is_a_day_the_ledger_can_order() {
        let today = today();
        assert_eq!(today.len(), 10, "{today}");
        assert_eq!(&today[4..5], "-");
        assert_eq!(&today[7..8], "-");
    }
}
