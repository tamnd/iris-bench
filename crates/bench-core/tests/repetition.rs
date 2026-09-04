//! The pilot, the variance decomposition, and where the budget goes.

use bench_core::{Error, Level, Pilot, plan};

/// A small deterministic generator so these tests say the same thing on every machine.
struct Lcg(u64);

impl Lcg {
    fn next_unit(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        // The top 24 bits, scaled into -0.5 ..= 0.5.
        f64::from((self.0 >> 40) as u32) / f64::from(1u32 << 24) - 0.5
    }
}

/// Builds a pilot where the variance is injected at exactly one level and the other two carry only
/// a small amount of unavoidable jitter.
fn pilot_with_variance_at(level: Level) -> Pilot {
    let mut rng = Lcg(12_345);
    let big = 100.0;
    let small = 1.0;

    let amount = |at: Level| if at == level { big } else { small };

    let mut samples = Vec::new();
    for _ in 0..4 {
        let build_offset = rng.next_unit() * amount(Level::Build);
        let mut build = Vec::new();
        for _ in 0..4 {
            let process_offset = rng.next_unit() * amount(Level::Process);
            let mut process = Vec::new();
            for _ in 0..8 {
                let jitter = rng.next_unit() * amount(Level::Iteration);
                process.push(1_000.0 + build_offset + process_offset + jitter);
            }
            build.push(process);
        }
        samples.push(build);
    }
    Pilot::new(samples).expect("the design above is balanced and big enough")
}

#[test]
fn the_decomposition_finds_the_level_the_variance_was_put_at() {
    for level in [Level::Build, Level::Process, Level::Iteration] {
        let components = pilot_with_variance_at(level).decompose();
        assert_eq!(
            components.dominant(),
            level,
            "variance was injected at {level} but the decomposition said {} ({components:?})",
            components.dominant()
        );
        assert!(
            components.share(level) > 0.5,
            "{level} should carry most of the variance, got {}",
            components.share(level)
        );
    }
}

#[test]
fn a_component_is_never_negative() {
    // Every sample identical, so every mean square is zero and the subtractions land on noise
    // around zero rather than on a real difference.
    let flat = vec![vec![vec![1_000.0; 4]; 3]; 2];
    let components = Pilot::new(flat).unwrap().decompose();
    assert!(components.build >= 0.0);
    assert!(components.process >= 0.0);
    assert!(components.iteration >= 0.0);
    assert!(components.share(Level::Build).abs() < f64::EPSILON);
}

#[test]
fn a_pilot_that_cannot_separate_the_levels_is_refused() {
    let one_build = vec![vec![vec![1.0, 2.0]; 2]];
    assert!(matches!(
        Pilot::new(one_build),
        Err(Error::PilotTooSmall { builds: 1, .. })
    ));

    let one_iteration = vec![vec![vec![1.0]; 2]; 2];
    assert!(matches!(
        Pilot::new(one_iteration),
        Err(Error::PilotTooSmall { iterations: 1, .. })
    ));

    assert!(matches!(
        Pilot::new(Vec::new()),
        Err(Error::PilotTooSmall { .. })
    ));
}

#[test]
fn a_ragged_design_is_refused() {
    let ragged = vec![vec![vec![1.0, 2.0], vec![3.0, 4.0]], vec![vec![5.0, 6.0]]];
    assert!(matches!(Pilot::new(ragged), Err(Error::Unbalanced)));

    let short_cell = vec![
        vec![vec![1.0, 2.0], vec![3.0, 4.0]],
        vec![vec![5.0, 6.0], vec![7.0]],
    ];
    assert!(matches!(Pilot::new(short_cell), Err(Error::Unbalanced)));
}

#[test]
fn a_broken_clock_does_not_get_into_a_pilot() {
    let bad = vec![vec![vec![1.0, f64::INFINITY], vec![3.0, 4.0]]; 2];
    assert!(matches!(Pilot::new(bad), Err(Error::NotFinite)));
}

#[test]
fn the_budget_goes_to_the_level_that_carries_the_variance() {
    let budget = 200;

    let iteration = plan(budget, Level::Iteration);
    assert!(iteration.iterations > iteration.processes);
    assert!(iteration.iterations > iteration.builds);

    let process = plan(budget, Level::Process);
    assert!(process.processes > process.iterations);
    assert!(process.processes > process.builds);

    let build = plan(budget, Level::Build);
    assert!(build.builds > build.processes);
    assert!(build.builds > build.iterations);
}

#[test]
fn a_plan_spends_roughly_the_budget_it_was_given() {
    for level in [Level::Build, Level::Process, Level::Iteration] {
        let p = plan(200, level);
        assert_eq!(p.total(), 200, "{level} plan spent {}", p.total());
    }
}

#[test]
fn a_tiny_budget_still_produces_a_runnable_plan() {
    let p = plan(0, Level::Iteration);
    assert!(p.builds >= 1);
    assert!(p.processes >= 1);
    assert!(p.iterations >= 1);
}
