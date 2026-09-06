//! What may and may not be said about a number measured on somebody else's specification.
//!
//! TPC-H and TPC-DS are trademarks of the Transaction Processing Performance Council, and the rules
//! for using them outside an audited run are specific. `docs/LICENSING.md` states them in prose.
//! This module is the same rules as code, because a rule that lives only in prose is a rule that
//! holds until the week somebody is in a hurry.
//!
//! The distinction the whole thing rests on: an unaudited run of a query set derived from a public
//! specification is a legitimate and common thing to publish, and calling it a TPC-H Result is not.
//! The two are one sentence apart and the sentence is load bearing. So there is no way to get the
//! wrong sentence out of this module. [`Family::label`] is the only name a TPC workload has here,
//! and it already contains the qualifier.

/// Which benchmark family a number came from, which is what decides how it may be described.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    /// Derived from the TPC-H specification.
    TpcH,
    /// Derived from the TPC-DS specification.
    TpcDs,
    /// Anything whose name nobody else owns.
    Independent,
}

/// The scale factors the TPC-H specification defines. Anything else is a deviation.
const TPCH_SCALES: [u32; 10] = [1, 10, 30, 100, 300, 1_000, 3_000, 10_000, 30_000, 100_000];

/// The scale factors the TPC-DS specification defines, which start where TPC-H's stop mattering.
const TPCDS_SCALES: [u32; 5] = [1_000, 3_000, 10_000, 30_000, 100_000];

/// The notice that has to appear on any page carrying a number from either TPC family.
const TPC_NOTICE: &str = "TPC, TPC-H and TPC-DS are trademarks of the Transaction Processing \
                          Performance Council. The numbers here are derived from the published \
                          specifications, are not audited, and are not TPC Results.";

impl Family {
    /// The family a workload identifier belongs to.
    ///
    /// Worked out from the identifier rather than declared alongside it, because a declaration is a
    /// thing somebody has to remember and this is exactly the rule that must not depend on
    /// remembering. Name a corpus `tpch-sf20` and it carries the TPC rules from that moment on,
    /// with nothing else to fill in.
    #[must_use]
    pub fn of(workload: &str) -> Self {
        let squashed: String = workload
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .flat_map(char::to_lowercase)
            .collect();
        if squashed.starts_with("tpcds") {
            Self::TpcDs
        } else if squashed.starts_with("tpch") {
            Self::TpcH
        } else {
            Self::Independent
        }
    }

    /// How a result from this family is named in output.
    ///
    /// For a TPC family this is the only name available, and it is not the trademark on its own.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::TpcH => "derived from TPC-H",
            Self::TpcDs => "derived from TPC-DS",
            Self::Independent => "",
        }
    }

    /// The notice that must appear on any page carrying one of these numbers.
    #[must_use]
    pub const fn notice(self) -> Option<&'static str> {
        match self {
            Self::TpcH | Self::TpcDs => Some(TPC_NOTICE),
            Self::Independent => None,
        }
    }

    /// Whether a scale factor is one the specification defines.
    ///
    /// Scale factor 20 is not, and is used here anyway because it is the size that stops fitting
    /// comfortably in cache on the workstation class. That is a good reason to run it and no reason
    /// at all to let it pass as a compliant scale factor, so it is marked.
    #[must_use]
    pub fn scale_is_compliant(self, scale: u32) -> bool {
        match self {
            Self::TpcH => TPCH_SCALES.contains(&scale),
            Self::TpcDs => TPCDS_SCALES.contains(&scale),
            // Nobody else's specification says which sizes are allowed, so nothing is a deviation.
            Self::Independent => true,
        }
    }
}

/// The scale factor a workload identifier names, if it names one.
///
/// Reads the digits after `sf`, which is how every corpus and every result file in this repository
/// spells it.
#[must_use]
pub fn scale_of(workload: &str) -> Option<u32> {
    let lowered = workload.to_ascii_lowercase();
    let after = lowered.split("sf").nth(1)?;
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// What a number that this harness did not measure came from.
///
/// Split into these two rather than left as a string, so that citing a TPC Result is something
/// somebody has to write down in as many words. Having written it down, they are refused. There is
/// no third case where an official result arrives without being called one.
#[derive(Clone, Debug)]
pub enum Cited {
    /// A number from a paper, a vendor post, or a leaderboard that is not TPC's.
    Published {
        /// Where it was published, for the citation.
        origin: String,
    },
    /// A number from TPC's own list of audited results.
    OfficialTpcResult {
        /// Where it was published, which does not help because this may not be used.
        origin: String,
    },
}

/// Why a row may not go on a page.
#[derive(Debug, thiserror::Error)]
pub enum WordingError {
    /// Somebody tried to put an audited TPC Result next to a number from here.
    #[error(
        "{workload}: {origin} is an official TPC Result, and a number from this harness may not be \
         compared against one. That is prohibited by TPC and by us, and it is prohibited because \
         an unaudited run and an audited one are not the same measurement however similar the \
         query text is"
    )]
    OfficialTpcResult {
        /// Which workload the row was for.
        workload: String,
        /// What was being cited.
        origin: String,
    },
    /// Somebody tried to file a skewed generator's output under the TPC numbers.
    #[error(
        "{workload} names a TPC family and a skew, and a skewed generator is a different benchmark. \
         Give it its own name, so that nothing merges its numbers into the TPC-H ones by matching \
         on a prefix"
    )]
    Skewed {
        /// Which workload the row was for.
        workload: String,
    },
}

/// The markers that make a workload a skewed one rather than the specification's own generator.
const SKEWS: [&str; 3] = ["skew", "zipf", "correlated"];

/// Checks a workload identifier against the rules that apply before a number is even attached.
///
/// # Errors
///
/// If the identifier names a TPC family and a skew, which is a different benchmark under a name
/// that would let it be merged into the TPC numbers.
pub fn check(workload: &str) -> Result<Family, WordingError> {
    let family = Family::of(workload);
    let lowered = workload.to_ascii_lowercase();
    if family != Family::Independent && SKEWS.iter().any(|skew| lowered.contains(skew)) {
        return Err(WordingError::Skewed {
            workload: workload.to_owned(),
        });
    }
    Ok(family)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_corpus_name_is_enough_to_carry_the_rules() {
        assert_eq!(Family::of("tpch-sf20"), Family::TpcH);
        assert_eq!(Family::of("TPC-H"), Family::TpcH);
        assert_eq!(Family::of("tpcds-sf1000"), Family::TpcDs);
        assert_eq!(Family::of("clickbench-hits"), Family::Independent);
    }

    #[test]
    fn tpc_ds_is_not_read_as_tpc_h_because_it_starts_the_same_way() {
        // The obvious implementation checks for `tpch` first and gets this wrong, since `tpcds`
        // does not begin with `tpch` but a sloppier match on `tpc` would put both in one family.
        assert_eq!(Family::of("tpcds-sf1000"), Family::TpcDs);
        assert_ne!(Family::of("tpcds-sf1000"), Family::TpcH);
    }

    #[test]
    fn a_tpc_workload_has_no_name_that_is_only_the_trademark() {
        // The point of the whole module in one assertion. There is no accessor that returns
        // "TPC-H" on its own, so no caller can render one by accident.
        assert_eq!(Family::TpcH.label(), "derived from TPC-H");
        assert!(Family::TpcH.label().starts_with("derived from"));
        assert!(Family::TpcDs.label().starts_with("derived from"));
    }

    #[test]
    fn a_tpc_number_carries_a_notice_and_an_independent_one_does_not() {
        assert!(Family::TpcH.notice().is_some());
        assert!(Family::TpcDs.notice().is_some());
        assert!(Family::Independent.notice().is_none());
    }

    #[test]
    fn scale_factor_twenty_is_a_deviation_and_scale_factor_one_is_not() {
        assert!(Family::TpcH.scale_is_compliant(1));
        assert!(!Family::TpcH.scale_is_compliant(20));
        // The corpora this repository actually has, so the rule is exercised by its own data.
        assert_eq!(scale_of("tpch-sf1"), Some(1));
        assert_eq!(scale_of("tpch-sf20"), Some(20));
        assert_eq!(scale_of("clickbench-hits"), None);
    }

    #[test]
    fn tpc_ds_has_its_own_list_of_scale_factors() {
        // Scale factor 1 is a compliant TPC-H size and is not a compliant TPC-DS one, so a single
        // shared list would quietly pass a deviation on one of the two.
        assert!(Family::TpcH.scale_is_compliant(1));
        assert!(!Family::TpcDs.scale_is_compliant(1));
        assert!(Family::TpcDs.scale_is_compliant(1_000));
    }

    #[test]
    fn nothing_is_a_deviation_for_a_benchmark_nobody_else_specifies() {
        assert!(Family::Independent.scale_is_compliant(20));
    }

    #[test]
    fn a_skewed_generator_under_a_tpc_name_is_refused() {
        let error = check("tpch-skew-sf1").unwrap_err();
        assert!(matches!(error, WordingError::Skewed { .. }));
        // Under its own name it is simply a different benchmark and none of this applies.
        assert_eq!(check("myskew-sf1").unwrap(), Family::Independent);
    }

    #[test]
    fn an_ordinary_tpc_workload_passes_the_check() {
        assert_eq!(check("tpch-sf20").unwrap(), Family::TpcH);
    }
}
