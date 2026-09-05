# Releasing

Nothing in this repository goes to crates.io. Every crate here carries `publish = false` and that is deliberate: this is a measuring instrument for one project rather than a library anybody should depend on, and a version on crates.io is a promise about an interface that this repository is not in a position to make. What a release is here is a tag and a set of notes, and what it marks is a milestone finishing.

## What a version means

While the major is zero, a minor tracks a completed milestone and a patch is everything in between. `v0.1.0` is B0. `v0.2.0` will be B1. A patch is cut when enough has accumulated between milestones to be worth a boundary, and it carries no promise beyond being a point somebody can name.

The reason the numbering follows the milestones rather than the code is that the milestones are the unit of work here. A reader who compares two minors and finds a small diff should read the milestone rather than the range, because a gate is often answered by a measurement that changes one document and nothing else.

## Where the version lives

Only in `[workspace.package]` in the root `Cargo.toml`, and in the `version` field of each path dependency in `[workspace.dependencies]`, which all carry the same number. Nothing else in the repository names a version, so a bump is one file and then `cargo update -w` to move the lockfile.

## Cutting one

Open a release pull request that bumps the version and updates the lockfile, and nothing else. A release that carries a change is a release whose notes have to explain two things at once, and the notes are the part of this that has value.

Merge it once CI is green, tag the merge commit, and push the tag. Tag the merge commit rather than the branch, because a version is one tree and a tag that points at a branch tip points at whatever landed next.

Then write the notes and publish the release. The notes say what the milestone answered, including where the answer was not the one the gate expected, with the numbers and the machine they came from. A release note here that only lists merged pull requests has failed at the one job it has.

## What is not automated

All of it, for now. There is no release workflow, because a repository that publishes nothing has no step that a person can get irreversibly wrong, and automating a tag push to save one command is not worth a workflow that has to be maintained. When a release starts producing an artifact somebody else consumes, that changes, and this section is where it will be written down.
