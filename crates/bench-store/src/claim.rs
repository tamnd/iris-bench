//! The claim ledger: what this project said it would test, and how it came out.
//!
//! A claim is registered before it is run, with the threshold that decides it and the instrument
//! that would settle it. Registration is the whole point. A threshold written after the number
//! exists is not a threshold, it is a description of the number, and the two are indistinguishable
//! on the page a reader eventually sees. So a claim carries the day it was registered and where the
//! threshold was written down, and [`Claim::decide`] refuses to grade a measurement taken before
//! that day rather than trusting whoever assembled the entry.
//!
//! ```
//! use bench_store::claim::{Attempt, Measured, Verdict, registry};
//!
//! let claim = registry().into_iter().find(|one| one.id == "C0002").expect("C0002 is registered");
//!
//! // What the windowed path actually came to, at the span iris ships, against a bar of three
//! // percent and an instrument whose own bias is nine.
//! let measured = Attempt::Measured(Measured {
//!     day: "2026-09-06".to_owned(),
//!     value: 1.4351,
//!     resolution: 0.0888,
//!     caveats: Vec::new(),
//! });
//!
//! assert_eq!(claim.decide(Some(&measured)).unwrap(), Verdict::NotReproduced);
//! assert_eq!(claim.decide(None).unwrap(), Verdict::Pending);
//! ```
//!
//! # The five words
//!
//! `REPRODUCED`, `REPRODUCED-WITH-CAVEAT`, `NOT-REPRODUCED`, `NOT-ATTEMPTABLE` and `PENDING`. The
//! set is closed and the spelling is fixed, which is the only reason it is worth having. A
//! vocabulary somebody can extend is a vocabulary that grows a gentler word every time a result
//! comes out badly, and the gentler word is always the one that gets quoted. [`Verdict`] parses
//! exactly these five and serialises as exactly these five, so the word in a file, the word on a
//! terminal and the word in the ledger are one string.
//!
//! None of the three that follow from a measurement is chosen by a person. [`Bar::decide`] is the
//! only thing that produces them, from the number, the threshold and what the instrument can
//! resolve.
//!
//! # A caveat cannot rescue a failure
//!
//! A measurement outside the bar is `NOT-REPRODUCED` whatever else was true about the run. Caveats
//! only ever turn a `REPRODUCED` into a `REPRODUCED-WITH-CAVEAT`, never a `NOT-REPRODUCED` into
//! anything. Softening a loss with a note about the conditions is exactly the move the fixed
//! vocabulary exists to prevent, and allowing a caveat to travel in both directions would hand it
//! back.
//!
//! # `NOT-ATTEMPTABLE` is about our circumstances
//!
//! Two different situations reach it and both are facts about us rather than criticism of anybody's
//! work. One is an artifact that cannot be obtained or cannot be run here, which arrives as
//! [`Attempt::Refused`] and has to carry a citation, because an unsupported assertion that
//! something was unavailable is not better than no entry. The other is a bar narrower than what the
//! instrument can resolve, where the honest answer is that this harness cannot settle the question
//! either way. The entry says which, and the verdict is the same word for both because both mean
//! the claim was not settled here.
//!
//! What is not `NOT-ATTEMPTABLE` is a claim whose instrument has not been written yet. That is a
//! gap in this repository's work, and dressing it as a verdict would put a finished looking row on
//! a page for something nobody has started. A caller asked to run one of those gets an error.
//!
//! # What the resolution is for
//!
//! An instrument that cannot tell two copies of the same thing apart to within nine percent cannot
//! honestly report that two different things are three percent apart. So a measurement carries what
//! its instrument resolves, measured in the same run rather than assumed, and a reading close
//! enough to the bar that the difference is inside that figure is not decided in either direction.
//!
//! The alternative, widening the bar until the instrument clears it, produces a number that looks
//! like a pass and means nothing, and it is the reason this rule is code here instead of a habit.

use std::fmt;
use std::str::FromStr;

/// Every claim this repository has registered, as the files that carry them.
///
/// One file per claim, committed, and the file is the registration. Adding an entry to this list is
/// a change to a public list of what the project intends to prove.
const REGISTERED: &[&str] = &[include_str!("../claims/C0002.toml")];

/// Every registered claim, parsed.
///
/// # Panics
///
/// If a carried claim does not parse, which is a bug in this crate rather than anything a caller
/// can do about it. The test below parses all of them.
#[must_use]
pub fn registry() -> Vec<Claim> {
    REGISTERED
        .iter()
        .map(|text| toml::from_str(text).expect("a carried claim has to parse"))
        .collect()
}

/// Looks a registered claim up by identifier, ignoring case.
///
/// Ignoring case because the identifiers are cited in prose as `C0002` and typed on a command line
/// as `c0002`, and refusing the second would be a papercut with nothing behind it.
#[must_use]
pub fn registered(id: &str) -> Option<Claim> {
    registry()
        .into_iter()
        .find(|claim| claim.id.eq_ignore_ascii_case(id))
}

/// One registered claim.
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Claim {
    /// The identifier other documents cite, such as `C0002`.
    pub id: String,
    /// What the claim is about, in one phrase.
    pub title: String,
    /// Whether a verdict here may be cited as evidence.
    pub purpose: Purpose,
    /// When the threshold was committed, and where.
    pub registered: Registration,
    /// What the reading is judged against.
    pub bar: Bar,
    /// What would be run to settle it.
    pub instrument: Instrument,
}

impl Claim {
    /// Whether a verdict on this claim may be cited as a claim anywhere.
    ///
    /// False for an exploratory registration, and that is fixed when the claim is registered. It is
    /// what stops a measurement taken to see what happens from being promoted to evidence once it
    /// comes out well.
    #[must_use]
    pub fn citable(&self) -> bool {
        self.purpose == Purpose::Confirmatory
    }

    /// Turns what was attempted into one of the five words.
    ///
    /// `None` is a claim nobody has run, which is `PENDING` and is meant to be visible as a gap in
    /// a committed file rather than as a number that quietly never appeared.
    ///
    /// # Errors
    ///
    /// If the entry cannot be graded at all: a measurement older than its own threshold, a day that
    /// is not a day, a registration that does not say where the threshold was written, a refusal
    /// with no citation, or a bar expressed as a fraction of zero. None of those is a result, so
    /// none of them is a verdict.
    pub fn decide(&self, attempt: Option<&Attempt>) -> Result<Verdict, Ungradable> {
        if self.registered.source.trim().is_empty() {
            return Err(Ungradable::Unregistered {
                claim: self.id.clone(),
            });
        }
        if !is_day(&self.registered.day) {
            return Err(Ungradable::NotADay {
                claim: self.id.clone(),
                which: "registered",
                given: self.registered.day.clone(),
            });
        }

        let Some(attempt) = attempt else {
            return Ok(Verdict::Pending);
        };

        match attempt {
            Attempt::Refused(refused) => {
                if refused.citation.trim().is_empty() {
                    return Err(Ungradable::NoCitation {
                        claim: self.id.clone(),
                    });
                }
                Ok(Verdict::NotAttemptable)
            }
            Attempt::Measured(measured) => {
                if !is_day(&measured.day) {
                    return Err(Ungradable::NotADay {
                        claim: self.id.clone(),
                        which: "measured",
                        given: measured.day.clone(),
                    });
                }
                // Both are `YYYY-MM-DD`, which orders the same way as text and as a date, and that
                // is the reason the format is checked above rather than taken on trust.
                if measured.day < self.registered.day {
                    return Err(Ungradable::Backdated {
                        claim: self.id.clone(),
                        registered: self.registered.day.clone(),
                        measured: measured.day.clone(),
                    });
                }
                self.bar
                    .decide(measured)
                    .ok_or(Ungradable::RelativeToNothing {
                        claim: self.id.clone(),
                    })
            }
        }
    }
}

/// Whether a verdict may be cited as evidence, fixed at registration.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    /// Registered to settle something, and citable.
    Confirmatory,
    /// Registered to find out what happens, and not citable as a claim anywhere.
    Exploratory,
}

/// When a threshold was committed, and where it can be read.
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Registration {
    /// The day, as `YYYY-MM-DD`.
    pub day: String,
    /// Where the threshold was written down, in enough detail to go and look: an issue, a commit,
    /// or a document and the commit that carried it.
    pub source: String,
}

/// What a reading is judged against, written before the reading exists.
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Bar {
    /// What is divided by what, so that the number has a meaning without the code in front of you.
    pub reading: String,
    /// The value the reading is compared against.
    pub target: f64,
    /// How far from the target the reading may sit, as a fraction of the target.
    pub tolerance: f64,
}

impl Bar {
    /// Grades one measurement, or `None` when the target is zero and a fraction of it means
    /// nothing.
    ///
    /// The three outcomes are the reading being inside the bar with room the instrument can see,
    /// outside it with room the instrument can see, or too close to the bar for this instrument to
    /// call. Caveats only separate the first into its two spellings.
    #[must_use]
    pub fn decide(&self, measured: &Measured) -> Option<Verdict> {
        if self.target.abs() < f64::EPSILON {
            return None;
        }
        let off = (measured.value - self.target).abs() / self.target.abs();
        if off + measured.resolution <= self.tolerance {
            Some(if measured.caveats.is_empty() {
                Verdict::Reproduced
            } else {
                Verdict::ReproducedWithCaveat
            })
        } else if off - measured.resolution > self.tolerance {
            Some(Verdict::NotReproduced)
        } else {
            Some(Verdict::NotAttemptable)
        }
    }
}

/// What would be run to settle a claim.
///
/// The variants are the instruments that exist. A claim naming one that does not fails to parse,
/// so an entry cannot sit in the ledger looking registered while nothing could ever settle it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Instrument {
    /// `iris-bench resident`, with the arguments the registration fixed.
    Resident(Resident),
}

/// The arguments `iris-bench resident` is run with to settle a claim, in bytes.
///
/// Bytes rather than the mebibytes and kibibytes the command line takes, because a registration is
/// read by whoever is checking the ordering years later and a unit in the field name is one fewer
/// thing for them to get wrong.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Resident {
    /// How large a file to scan.
    pub size: u64,
    /// How much address space the window reserves.
    pub span: u64,
    /// How large each range a scan asks for is.
    pub chunk: u64,
    /// How many pairs of measurements to take.
    pub pairs: u32,
    /// How many scans of each side to run before recording anything.
    pub warmup: u32,
}

/// What a run of a claim produced.
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Attempt {
    /// A number came back.
    Measured(Measured),
    /// Nothing could be run, for a reason about our circumstances.
    Refused(Refused),
}

/// A number and everything needed to grade it.
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Measured {
    /// The day it was taken, as `YYYY-MM-DD`.
    pub day: String,
    /// The reading, in whatever [`Bar::reading`] says it is.
    pub value: f64,
    /// What the instrument can resolve, as a fraction of the target, measured in the same run.
    ///
    /// Zero for a deterministic reading such as a compression ratio, where re-running produces the
    /// same number and there is nothing for a control to say.
    pub resolution: f64,
    /// What was true about the run that a reader would want next to a pass.
    pub caveats: Vec<Caveat>,
}

/// Why nothing could be run.
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Refused {
    /// The day the attempt was made, as `YYYY-MM-DD`.
    pub day: String,
    /// What was missing, worded as a fact about our circumstances.
    pub what: String,
    /// Where it was looked for, so that the refusal is checkable rather than asserted.
    pub citation: String,
}

/// Something true about a run that a reader would want next to a pass.
///
/// Each of these is a fact the harness knows about the run rather than a judgement somebody made
/// afterwards, which is what keeps `REPRODUCED-WITH-CAVEAT` from becoming the polite spelling of a
/// result nobody liked.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Caveat {
    /// The machine failed an eligibility gate and the run was taken anyway.
    GatesOverridden,
    /// The artifact was run on a platform its authors did not say they tested.
    UntestedPlatform,
    /// Part of the configuration is in nobody's documentation and was guessed.
    ConfigurationGuessed,
}

impl Caveat {
    /// What it means, in one line, for putting next to a verdict.
    #[must_use]
    pub fn what(self) -> &'static str {
        match self {
            Self::GatesOverridden => {
                "the machine failed an eligibility gate and the run was taken anyway"
            }
            Self::UntestedPlatform => {
                "the artifact was run on a platform its authors did not say they tested"
            }
            Self::ConfigurationGuessed => {
                "part of the configuration is in nobody's documentation and was guessed"
            }
        }
    }
}

/// One of five words, and there is no sixth.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
pub enum Verdict {
    /// The reading met the threshold that was registered before it existed.
    #[serde(rename = "REPRODUCED")]
    Reproduced,
    /// The reading met it, and something about the run belongs next to that.
    #[serde(rename = "REPRODUCED-WITH-CAVEAT")]
    ReproducedWithCaveat,
    /// The reading did not meet it.
    #[serde(rename = "NOT-REPRODUCED")]
    NotReproduced,
    /// It was not settled here, and the entry says which of the two reasons applies.
    #[serde(rename = "NOT-ATTEMPTABLE")]
    NotAttemptable,
    /// Registered, and nobody has run it.
    #[serde(rename = "PENDING")]
    Pending,
}

impl Verdict {
    /// The word, which is the same string everywhere this verdict appears.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Reproduced => "REPRODUCED",
            Self::ReproducedWithCaveat => "REPRODUCED-WITH-CAVEAT",
            Self::NotReproduced => "NOT-REPRODUCED",
            Self::NotAttemptable => "NOT-ATTEMPTABLE",
            Self::Pending => "PENDING",
        }
    }

    /// All five, in the order they are written in `docs/CLAIMS.md`.
    #[must_use]
    pub fn all() -> [Self; 5] {
        [
            Self::Reproduced,
            Self::ReproducedWithCaveat,
            Self::NotReproduced,
            Self::NotAttemptable,
            Self::Pending,
        ]
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.word())
    }
}

impl FromStr for Verdict {
    type Err = NotAVerdict;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::all()
            .into_iter()
            .find(|verdict| verdict.word() == text)
            .ok_or_else(|| NotAVerdict {
                given: text.to_owned(),
            })
    }
}

/// A word that is not one of the five.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
#[error(
    "{given} is not a verdict, and the five are REPRODUCED, REPRODUCED-WITH-CAVEAT, \
     NOT-REPRODUCED, NOT-ATTEMPTABLE and PENDING"
)]
pub struct NotAVerdict {
    /// What was given instead.
    pub given: String,
}

/// An entry that cannot be turned into a verdict at all.
///
/// Every one of these is a fault in the entry rather than an outcome of the claim, which is why
/// they are errors and not a sixth word.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum Ungradable {
    /// The registration does not say where the threshold was written down.
    #[error(
        "{claim} does not say where its threshold was registered, so there is nothing to check the \
         ordering against and it cannot be graded"
    )]
    Unregistered {
        /// Which claim.
        claim: String,
    },
    /// A day is not a day, so the ordering cannot be checked.
    #[error("{claim} gives {given} as the day it was {which}, and a day here is YYYY-MM-DD")]
    NotADay {
        /// Which claim.
        claim: String,
        /// Which of the two days, `registered` or `measured`.
        which: &'static str,
        /// What was given.
        given: String,
    },
    /// The measurement is older than the threshold that judges it.
    #[error(
        "{claim} was measured on {measured} and registered on {registered}, so the threshold was \
         not committed before the number existed and this is not a pre-registered result"
    )]
    Backdated {
        /// Which claim.
        claim: String,
        /// The day the threshold was committed.
        registered: String,
        /// The day the number was taken.
        measured: String,
    },
    /// A refusal with nothing behind it.
    #[error(
        "{claim} was recorded as unattemptable with no citation, and where the artifact was looked \
         for is the part of that a reader cannot supply"
    )]
    NoCitation {
        /// Which claim.
        claim: String,
    },
    /// A tolerance expressed as a fraction of zero.
    #[error("{claim} has a target of zero, and a tolerance is a fraction of the target")]
    RelativeToNothing {
        /// Which claim.
        claim: String,
    },
}

/// Whether a string is a `YYYY-MM-DD` day.
///
/// Shape only. A day this accepts still orders correctly against another one it accepts, which is
/// all [`Claim::decide`] asks of it, and a full calendar here would be a date library nobody needs.
fn is_day(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(at, byte)| at == 4 || at == 7 || byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The claim the rest of these grade against, with the bar C0002 registered.
    fn claim() -> Claim {
        registered("C0002").expect("C0002 is registered")
    }

    fn measured(value: f64, resolution: f64, caveats: Vec<Caveat>) -> Attempt {
        Attempt::Measured(Measured {
            day: "2026-09-06".to_owned(),
            value,
            resolution,
            caveats,
        })
    }

    #[test]
    fn every_carried_claim_parses_and_has_an_identifier() {
        let claims = registry();
        assert!(!claims.is_empty());
        for claim in &claims {
            assert!(claim.id.starts_with('C'), "{}", claim.id);
            assert!(!claim.title.is_empty(), "{}", claim.id);
            assert!(!claim.bar.reading.is_empty(), "{}", claim.id);
        }
    }

    #[test]
    fn a_claim_is_found_however_its_identifier_is_typed() {
        assert_eq!(registered("c0002").map(|claim| claim.id), Some(claim().id));
        assert!(registered("C9999").is_none());
    }

    #[test]
    fn the_registered_bar_is_the_one_the_ledger_prints() {
        // If somebody widens the bar in the file, this is the test that has to be edited too, which
        // is the point at which moving a threshold stops being a quiet change.
        let claim = claim();
        assert!((claim.bar.tolerance - 0.03).abs() < 1e-12);
        assert!((claim.bar.target - 1.0).abs() < 1e-12);
        assert_eq!(claim.purpose, Purpose::Confirmatory);
        assert!(claim.citable());
    }

    #[test]
    fn a_claim_nobody_has_run_is_pending() {
        assert_eq!(claim().decide(None).unwrap(), Verdict::Pending);
    }

    #[test]
    fn the_windowed_path_at_the_span_iris_ships_did_not_reproduce() {
        // The numbers C0002 actually came back with. Forty three percent off a three percent bar is
        // outside it by far more than the nine percent the instrument cannot see, so this is
        // decided rather than left open.
        let verdict = claim()
            .decide(Some(&measured(1.4351, 0.0888, Vec::new())))
            .unwrap();
        assert_eq!(verdict, Verdict::NotReproduced);
    }

    #[test]
    fn a_reading_the_instrument_cannot_tell_from_the_bar_is_not_decided_either_way() {
        // The same claim at a span that holds the whole file: three percent off a three percent bar,
        // with a control that puts two copies of one buffer nine percent apart. Calling that a pass
        // or a failure would be reporting the harness.
        let verdict = claim()
            .decide(Some(&measured(0.9672, 0.0888, Vec::new())))
            .unwrap();
        assert_eq!(verdict, Verdict::NotAttemptable);
    }

    #[test]
    fn a_caveat_cannot_turn_a_failure_into_anything_softer() {
        let with = claim()
            .decide(Some(&measured(1.4351, 0.0, vec![Caveat::GatesOverridden])))
            .unwrap();
        let without = claim()
            .decide(Some(&measured(1.4351, 0.0, Vec::new())))
            .unwrap();
        assert_eq!(with, Verdict::NotReproduced);
        assert_eq!(with, without);
    }

    #[test]
    fn a_caveat_is_the_whole_difference_between_the_two_spellings_of_a_pass() {
        let clean = claim()
            .decide(Some(&measured(1.01, 0.0, Vec::new())))
            .unwrap();
        let noted = claim()
            .decide(Some(&measured(1.01, 0.0, vec![Caveat::UntestedPlatform])))
            .unwrap();
        assert_eq!(clean, Verdict::Reproduced);
        assert_eq!(noted, Verdict::ReproducedWithCaveat);
    }

    #[test]
    fn a_measurement_older_than_its_own_threshold_is_not_graded() {
        let early = Attempt::Measured(Measured {
            day: "2026-09-03".to_owned(),
            value: 1.0,
            resolution: 0.0,
            caveats: Vec::new(),
        });
        let error = claim().decide(Some(&early)).unwrap_err();
        assert!(
            matches!(error, Ungradable::Backdated { .. }),
            "{error:?}, and a threshold written after the number is not a threshold"
        );
    }

    #[test]
    fn an_unattemptable_claim_has_to_say_where_the_artifact_was_looked_for() {
        let refused = |citation: &str| {
            Attempt::Refused(Refused {
                day: "2026-09-06".to_owned(),
                what: "the artifact is not published anywhere we could find".to_owned(),
                citation: citation.to_owned(),
            })
        };

        let error = claim().decide(Some(&refused("   "))).unwrap_err();
        assert!(matches!(error, Ungradable::NoCitation { .. }), "{error:?}");

        let cited = refused("the paper's artifact link, dead as of the day above");
        assert_eq!(
            claim().decide(Some(&cited)).unwrap(),
            Verdict::NotAttemptable
        );
    }

    #[test]
    fn a_registration_that_does_not_say_where_cannot_be_graded() {
        let mut claim = claim();
        claim.registered.source = String::new();
        let error = claim.decide(None).unwrap_err();
        assert!(
            matches!(error, Ungradable::Unregistered { .. }),
            "{error:?}"
        );
    }

    #[test]
    fn a_day_that_is_not_a_day_is_refused_rather_than_compared() {
        // Two strings that are not both YYYY-MM-DD do not order the way their dates do, so this is
        // checked instead of letting the ordering check quietly answer with nonsense.
        assert!(is_day("2026-09-04"));
        assert!(!is_day("2026-9-4"));
        assert!(!is_day("4 September 2026"));
        assert!(!is_day(""));

        let mut claim = claim();
        claim.registered.day = "September 2026".to_owned();
        let error = claim.decide(None).unwrap_err();
        assert!(matches!(error, Ungradable::NotADay { .. }), "{error:?}");
    }

    #[test]
    fn a_tolerance_that_is_a_fraction_of_zero_is_refused() {
        let mut claim = claim();
        claim.bar.target = 0.0;
        let error = claim
            .decide(Some(&measured(1.0, 0.0, Vec::new())))
            .unwrap_err();
        assert!(
            matches!(error, Ungradable::RelativeToNothing { .. }),
            "{error:?}"
        );
    }

    #[test]
    fn the_five_words_round_trip_and_nothing_else_parses() {
        for verdict in Verdict::all() {
            assert_eq!(verdict.word().parse::<Verdict>().unwrap(), verdict);
            assert_eq!(verdict.to_string(), verdict.word());
            // The stored spelling and the printed one are one string, so a file cannot disagree
            // with a terminal about what happened.
            let json = serde_json::to_string(&verdict).unwrap();
            assert_eq!(json, format!("\"{}\"", verdict.word()));
        }
        for not in ["reproduced", "MOSTLY-REPRODUCED", "INCONCLUSIVE", ""] {
            assert!(not.parse::<Verdict>().is_err(), "{not} parsed");
        }
    }

    #[test]
    fn the_instrument_a_claim_names_carries_what_it_would_be_run_with() {
        let Instrument::Resident(resident) = claim().instrument;
        assert_eq!(resident.size, 256 * 1024 * 1024);
        assert_eq!(resident.span, 4 * 1024 * 1024);
        assert_eq!(resident.chunk, 256 * 1024);
        assert_eq!(resident.pairs, 60);
    }

    #[test]
    fn an_instrument_this_repository_does_not_have_fails_to_parse() {
        // A claim that named an instrument nobody wrote would sit in the ledger looking registered
        // while nothing could ever settle it, so it is refused at the point the file is read.
        let text = "\
id = \"C0000\"
title = \"something nobody can measure\"
purpose = \"confirmatory\"

[registered]
day = \"2026-09-04\"
source = \"nowhere\"

[bar]
reading = \"a thing over another thing\"
target = 1.0
tolerance = 0.03

[instrument.telepathy]
";
        assert!(toml::from_str::<Claim>(text).is_err());
    }
}
