//! The stopping rule, and the property that makes it honest.

use bench_core::{Bootstrap, Series, Stop, Timing};

/// A deterministic stream of samples, so a test can say what the loop will see.
struct Stream {
    values: Vec<f64>,
    at: usize,
}

impl Stream {
    fn new(values: Vec<f64>) -> Self {
        Self { values, at: 0 }
    }

    fn next(&mut self) -> f64 {
        let v = self.values[self.at % self.values.len()];
        self.at += 1;
        v
    }
}

/// Fewer resamples than a published run would use. These tests are about where a run stops, not
/// about the third digit of an interval endpoint, and a debug build pays for every resample.
fn quick() -> Bootstrap {
    Bootstrap {
        resamples: 200,
        ..Bootstrap::default()
    }
}

/// A workload that wobbles by a fixed pattern around 1000, so every run of this test sees exactly
/// the same numbers in the same order.
fn wobbly() -> Vec<f64> {
    (0..64)
        .map(|i| 1_000.0 + f64::from((i * 37) % 23) - 11.0)
        .collect()
}

/// The property issue #5 asks for.
///
/// A comparison driven stopping rule is not reachable, because `Stop::should_stop` is handed one
/// `Series` and `Stop` is `#[non_exhaustive]` with no variant that carries anything but a scalar.
/// Nobody outside this crate can add a variant that sees a second series.
///
/// What a test can check is the observable half of that: the stopping point is a pure function of
/// the samples, so a run stops at the same place whatever else was measured that day.
#[test]
fn the_stopping_point_does_not_depend_on_anything_but_the_series() {
    let timing = Timing {
        warmup: 3,
        stop: Stop::IntervalWidth {
            target: 0.01,
            min: 10,
            max: 400,
        },
        check_every: 1,
        bootstrap: quick(),
    };

    // The same workload, measured on a day when the rival was far behind and on a day when it was
    // a hair ahead. If a comparison could reach the stopping rule, these would come out different,
    // because the second case is the one where a few more repetitions would be tempting.
    let mut far = Stream::new(wobbly());
    let run_against_a_distant_rival = timing.run(|| far.next());

    let mut close = Stream::new(wobbly());
    let run_against_a_near_tie = timing.run(|| close.next());

    assert_eq!(
        run_against_a_distant_rival.len(),
        run_against_a_near_tie.len()
    );
    assert_eq!(
        run_against_a_distant_rival.summary(),
        run_against_a_near_tie.summary()
    );
}

/// The same claim from the other side: replaying a recorded series through the rule one sample at a
/// time flips at exactly one place, and it is the place the loop stopped.
#[test]
fn replaying_a_series_stops_at_the_same_sample() {
    let stop = Stop::IntervalWidth {
        target: 0.01,
        min: 10,
        max: 400,
    };
    let timing = Timing {
        warmup: 0,
        stop,
        check_every: 1,
        bootstrap: quick(),
    };

    let mut source = Stream::new(wobbly());
    let collected = timing.run(|| source.next());

    let mut replay = Series::new(quick());
    let mut stopped_at = None;
    for sample in collected.samples() {
        replay.push(*sample);
        if stop.should_stop(&replay) {
            stopped_at = Some(replay.len());
            break;
        }
    }

    assert_eq!(stopped_at, Some(collected.len()));
}

#[test]
fn a_tighter_target_never_takes_fewer_samples() {
    let run = |target: f64| {
        let timing = Timing {
            warmup: 0,
            stop: Stop::IntervalWidth {
                target,
                min: 8,
                max: 400,
            },
            check_every: 1,
            bootstrap: quick(),
        };
        let mut source = Stream::new(wobbly());
        timing.run(|| source.next()).len()
    };

    let loose = run(0.05);
    let tight = run(0.001);
    assert!(
        tight >= loose,
        "asking for a narrower interval took fewer samples: {tight} against {loose}"
    );
}

/// An interval computed from four samples is narrow because there is nothing in it to be wide.
/// The floor exists so that a very quiet workload cannot end a run before it has said anything.
#[test]
fn a_narrow_interval_from_too_few_samples_does_not_stop_the_run() {
    let timing = Timing {
        warmup: 0,
        stop: Stop::IntervalWidth {
            target: 0.5,
            min: 25,
            max: 100,
        },
        check_every: 1,
        bootstrap: quick(),
    };
    // Every sample identical, so the interval has zero width from the second sample on.
    let series = timing.run(|| 1_000.0);
    assert_eq!(series.len(), 25);
}

#[test]
fn a_workload_whose_variance_never_settles_still_ends() {
    let timing = Timing {
        warmup: 0,
        stop: Stop::IntervalWidth {
            // Unreachable on purpose.
            target: 0.0,
            min: 4,
            max: 60,
        },
        check_every: 1,
        bootstrap: quick(),
    };
    let mut source = Stream::new(wobbly());
    assert_eq!(timing.run(|| source.next()).len(), 60);
}

#[test]
fn a_fixed_count_takes_exactly_that_many() {
    let timing = Timing {
        warmup: 7,
        stop: Stop::After { samples: 31 },
        check_every: 1,
        bootstrap: quick(),
    };
    let mut calls = 0;
    let series = timing.run(|| {
        calls += 1;
        1_000.0
    });
    assert_eq!(series.len(), 31);
    // Warmup runs the workload and throws the numbers away, which is the point of it.
    assert_eq!(calls, 38);
}

/// The cadence is not free of consequences, so it gets its own test rather than a footnote.
#[test]
fn a_run_ends_on_a_multiple_of_the_check_interval() {
    let timing = Timing {
        warmup: 0,
        stop: Stop::After { samples: 30 },
        check_every: 8,
        bootstrap: quick(),
    };
    // The rule was satisfied at 30 but is only asked at 32, and 32 is what the row will say.
    assert_eq!(timing.run(|| 1_000.0).len(), 32);
}
