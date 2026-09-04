## What this changes

<!-- One paragraph. -->

## Why

<!-- Link the issue or the milestone gate this moves. -->

## Checklist

- [ ] `cargo fmt --all --check` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] `python3 ci/discipline.py` passes

## If this adds or changes a driver

- [ ] `CONFIG.md` names where the configuration came from, with a link
- [ ] Deviations from the recommended configuration each have a reason
- [ ] Rejected alternatives are recorded
- [ ] The review date is today
- [ ] The driver depends on `bench-driver` and nothing else in the tree

## If this changes how a measurement is taken

- [ ] `METHODOLOGY_VERSION` is bumped
- [ ] The change is described in the pull request, because change point detection resets at this boundary

## If this adds or changes a published number

- [ ] The confidence interval and the sample size are given, not only the median
- [ ] The hardware class is given, and it is a class and not a machine name
- [ ] If it makes another system look bad, its maintainers have been given the bundle and the two week window
