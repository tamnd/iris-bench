//! The summary: the median, the interval around it, and the minimum that is kept out of the way.

use bench_core::{Bootstrap, coefficient_of_variation, paired_ratio, summarise};

fn boot() -> Bootstrap {
    Bootstrap::default()
}

#[test]
fn nothing_has_no_summary() {
    assert!(summarise(&[], boot()).is_none());
}

#[test]
fn the_median_is_the_middle_either_way_round() {
    let odd = summarise(&[3.0, 1.0, 2.0], boot()).unwrap();
    assert!((odd.median - 2.0).abs() < f64::EPSILON);

    let even = summarise(&[4.0, 1.0, 2.0, 3.0], boot()).unwrap();
    assert!((even.median - 2.5).abs() < f64::EPSILON);
}

#[test]
fn the_interval_brackets_the_median() {
    let samples: Vec<f64> = (1..=101).map(f64::from).collect();
    let s = summarise(&samples, boot()).unwrap();

    assert!((s.median - 51.0).abs() < f64::EPSILON);
    assert!(s.lo <= s.median, "{} is above the median", s.lo);
    assert!(s.hi >= s.median, "{} is below the median", s.hi);
    assert!((s.confidence - 0.95).abs() < f64::EPSILON);
}

#[test]
fn more_samples_narrow_the_interval() {
    // The same distribution twice, once with four times as many draws.
    let short: Vec<f64> = (0..40).map(|i| f64::from(i % 20) + 100.0).collect();
    let long: Vec<f64> = (0..160).map(|i| f64::from(i % 20) + 100.0).collect();

    let a = summarise(&short, boot()).unwrap();
    let b = summarise(&long, boot()).unwrap();
    assert!(
        b.relative_width() <= a.relative_width(),
        "four times the samples gave a wider interval: {} against {}",
        b.relative_width(),
        a.relative_width()
    );
}

/// The minimum is a statistic about the luckiest run. It is carried in its own column so that
/// nothing downstream can mistake it for a typical value.
#[test]
fn the_minimum_is_kept_out_of_the_headline() {
    // One very fast run and a lot of ordinary ones, which is what a lucky scheduling window looks
    // like in real data.
    let mut samples = vec![1_000.0; 99];
    samples.push(400.0);
    let s = summarise(&samples, boot()).unwrap();

    assert!((s.min - 400.0).abs() < f64::EPSILON);
    assert!((s.median - 1_000.0).abs() < f64::EPSILON);
    assert!(
        s.lo >= 900.0,
        "one lucky run dragged the interval to {}",
        s.lo
    );
}

#[test]
fn one_sample_admits_that_it_is_one_sample() {
    let s = summarise(&[1_234.0], boot()).unwrap();
    assert_eq!(s.n, 1);
    // Not a zero width interval pretending to be certainty. The `n` in the row is what tells a
    // reader what this is worth.
    assert!((s.lo - s.hi).abs() < f64::EPSILON);
    assert!((s.lo - 1_234.0).abs() < f64::EPSILON);
}

#[test]
fn the_same_samples_give_the_same_interval_every_time() {
    let samples: Vec<f64> = (0..77).map(|i| f64::from((i * 13) % 31) + 500.0).collect();
    assert_eq!(summarise(&samples, boot()), summarise(&samples, boot()));
}

#[test]
fn a_different_seed_is_an_independent_check_and_not_a_different_answer() {
    let samples: Vec<f64> = (0..201).map(|i| f64::from((i * 7) % 53) + 500.0).collect();
    let a = summarise(&samples, boot()).unwrap();
    let b = summarise(&samples, Bootstrap { seed: 99, ..boot() }).unwrap();

    assert!((a.median - b.median).abs() < f64::EPSILON);
    // The intervals are drawn from different resamples, so they are allowed to differ, but not by
    // much or the resample count is too low to be useful.
    let drift = (a.lo - b.lo).abs().max((a.hi - b.hi).abs());
    assert!(drift <= 2.0, "the interval moved by {drift} on a reseed");
}

#[test]
#[should_panic(expected = "not a finite number")]
fn a_broken_clock_is_not_summarised() {
    let _ = summarise(&[1.0, f64::NAN, 3.0], boot());
}

#[test]
fn a_machine_that_always_answers_the_same_has_no_spread() {
    let cv = coefficient_of_variation(&[1_000.0; 16]).unwrap();
    assert!(cv.abs() < f64::EPSILON, "a flat series came out at {cv}");
}

#[test]
fn the_spread_is_a_fraction_of_the_answer_and_not_a_duration() {
    // The same shape twice, one a thousand times slower than the other. A machine that is slower
    // and no less steady has the same noise floor, and a statistic that said otherwise would put
    // every fast machine at the top of the table for the wrong reason.
    let quick: Vec<f64> = (0..40).map(|i| 100.0 + f64::from(i % 4)).collect();
    let slow: Vec<f64> = quick.iter().map(|s| s * 1_000.0).collect();

    let a = coefficient_of_variation(&quick).unwrap();
    let b = coefficient_of_variation(&slow).unwrap();
    assert!((a - b).abs() < 1e-12, "{a} against {b}");
}

#[test]
fn a_known_series_gives_the_textbook_answer() {
    // Two, four and six. The mean is four and the sample standard deviation is two, so the
    // coefficient of variation is a half. Worth having one case a reader can check by hand rather
    // than only against the implementation that produced it.
    let cv = coefficient_of_variation(&[2.0, 4.0, 6.0]).unwrap();
    assert!((cv - 0.5).abs() < 1e-12, "{cv}");
}

#[test]
fn one_sample_has_no_spread_to_report() {
    assert!(coefficient_of_variation(&[1_234.0]).is_none());
    assert!(coefficient_of_variation(&[]).is_none());
}

#[test]
fn a_clock_that_did_not_move_is_not_infinitely_noisy() {
    // Dividing by a mean of zero would report this as the noisiest machine ever measured, when what
    // it means is that the workload took no time the clock could see.
    assert!(coefficient_of_variation(&[0.0, 0.0, 0.0]).is_none());
}

#[test]
fn nothing_has_no_ratio() {
    assert!(paired_ratio(&[], &[], boot()).is_none());
}

#[test]
fn two_series_that_are_the_same_come_out_at_parity() {
    let series: Vec<f64> = (0..60).map(|i| 100.0 + f64::from(i % 7)).collect();

    let ratio = paired_ratio(&series, &series, boot()).unwrap();
    assert!((ratio.ratio - 1.0).abs() < 1e-12, "{}", ratio.ratio);
    assert!(ratio.within(0.03), "{ratio:?}");
}

#[test]
fn a_known_pair_gives_the_ratio_of_the_medians() {
    // The second series is the first with a tenth added to every sample, so the ratio of the
    // medians is exactly eleven tenths whichever samples a resample happens to draw.
    let quick: Vec<f64> = (0..40).map(|i| 100.0 + f64::from(i % 5)).collect();
    let slow: Vec<f64> = quick.iter().map(|s| s * 1.1).collect();

    let ratio = paired_ratio(&slow, &quick, boot()).unwrap();
    assert!((ratio.ratio - 1.1).abs() < 1e-12, "{}", ratio.ratio);
    assert!((ratio.lo - 1.1).abs() < 1e-12, "{}", ratio.lo);
    assert!((ratio.hi - 1.1).abs() < 1e-12, "{}", ratio.hi);
    assert!(!ratio.within(0.03), "{ratio:?}");
}

#[test]
fn pairing_survives_the_bootstrap() {
    // Both series drift upward together, which is what a machine warming up during a run looks
    // like, and the pairs are two percent apart the whole way through. A comparison that keeps the
    // pairing sees two percent. One that resampled the two series independently would see the
    // drift as spread and report an interval several times too wide.
    let base: Vec<f64> = (0..200).map(|i| 100.0 + f64::from(i)).collect();
    let other: Vec<f64> = base.iter().map(|s| s * 1.02).collect();

    let ratio = paired_ratio(&other, &base, boot()).unwrap();
    assert!((ratio.hi - ratio.lo) < 1e-9, "{ratio:?}");
    assert!(ratio.within(0.03), "{ratio:?}");
}

#[test]
fn an_interval_that_straddles_the_bar_does_not_pass_it() {
    // The point estimate is inside three percent and the interval is not. A gate that read the
    // ratio alone would pass this, and it is exactly the case a gate exists to catch.
    let base: Vec<f64> = (0..30).map(|i| 100.0 + f64::from(i % 3)).collect();
    let other: Vec<f64> = base
        .iter()
        .enumerate()
        .map(|(i, s)| if i % 2 == 0 { s * 0.9 } else { s * 1.15 })
        .collect();

    let ratio = paired_ratio(&other, &base, boot()).unwrap();
    assert!(!ratio.within(0.03), "{ratio:?}");
}

#[test]
fn one_pair_reports_itself_and_says_so_in_n() {
    let ratio = paired_ratio(&[220.0], &[200.0], boot()).unwrap();
    assert_eq!(ratio.n, 1);
    assert!((ratio.ratio - 1.1).abs() < 1e-12, "{}", ratio.ratio);
    assert!((ratio.lo - ratio.hi).abs() < f64::EPSILON, "{ratio:?}");
}

#[test]
fn a_denominator_of_zero_is_not_a_ratio() {
    assert!(paired_ratio(&[1.0, 2.0], &[0.0, 0.0], boot()).is_none());
}

#[test]
#[should_panic(expected = "these are not pairs")]
fn unequal_lengths_are_not_pairs() {
    let _ = paired_ratio(&[1.0, 2.0], &[1.0], boot());
}
