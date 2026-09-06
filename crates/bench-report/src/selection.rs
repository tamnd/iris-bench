//! Which part of a corpus a number was taken from, for the corpora that come in more than one.
//!
//! Public BI is the case this exists for. The full benchmark is 206 tables and much of the encoding
//! literature measures a 36 table subset, so two numbers can both be honestly labelled Public BI
//! and mean different things. A compression ratio over the subset is not a compression ratio over
//! the full set, and the gap between them is large enough to reverse an ordering.
//!
//! So a row never says only Public BI. [`Selection::of`] reads which part from the workload
//! identifier, [`Selection::label`] is the only name available and it always names the part, and a
//! page carrying rows from two parts of one corpus says in as many words that they are not
//! comparable. None of that depends on whoever writes the page knowing about any of it.

/// Which part of a corpus that is published in more than one part a number came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Selection {
    /// The whole of a corpus that also has a named part in circulation.
    Whole {
        /// The corpus, as it is written when the part is not being named.
        corpus: &'static str,
        /// How many members the whole has, which is the thing that distinguishes it.
        members: &'static str,
    },
    /// A named part of such a corpus.
    Part {
        /// The corpus the part belongs to, which is what makes two parts comparable or not.
        corpus: &'static str,
        /// The part, named the way it is named in the work that uses it.
        name: &'static str,
    },
    /// A corpus that is not published in parts, which is nearly all of them.
    Undivided,
}

/// The notice a page carrying two parts of one corpus has to print.
const MIXED_NOTICE: &str = "This page carries numbers from more than one part of the same corpus. \
                            They are not comparable with each other, because they were measured \
                            over different data, and no row here is a comparison against another.";

impl Selection {
    /// Which part a workload identifier names.
    ///
    /// Read from the identifier rather than declared beside it, for the same reason the TPC family
    /// is. A declaration is a thing somebody has to remember, and the case worth catching is the
    /// one where a number was labelled in a hurry.
    #[must_use]
    pub fn of(workload: &str) -> Self {
        let squashed: String = workload
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .flat_map(char::to_lowercase)
            .collect();
        if !squashed.starts_with("publicbi") {
            return Self::Undivided;
        }
        // The subset is named for the 36 tables in it, so the identifier ends in the count. Checked
        // before the bare name, because the bare name is a prefix of it.
        if squashed.ends_with("36") {
            Self::Part {
                corpus: "Public BI",
                name: "36 table subset",
            }
        } else {
            Self::Whole {
                corpus: "Public BI",
                members: "all 206 tables",
            }
        }
    }

    /// The corpus this selection belongs to, when it belongs to one that has parts.
    ///
    /// Two selections sharing this are two measurements of the same benchmark over different data,
    /// which is the pair that must never be read as a comparison.
    #[must_use]
    pub const fn corpus(self) -> Option<&'static str> {
        match self {
            Self::Whole { corpus, .. } | Self::Part { corpus, .. } => Some(corpus),
            Self::Undivided => None,
        }
    }

    /// How this selection is named in output.
    ///
    /// Empty for a corpus that has no parts, so the workload identifier stands on its own. For one
    /// that does, this is the only name available and it names the part, so there is no spelling
    /// that reads as the whole benchmark when it is not.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Whole { members, .. } => members,
            Self::Part { name, .. } => name,
            Self::Undivided => "",
        }
    }

    /// The notice a page has to carry once it holds two different parts of one corpus.
    #[must_use]
    pub const fn mixed_notice() -> &'static str {
        MIXED_NOTICE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_subset_is_read_off_the_name() {
        assert_eq!(
            Selection::of("public-bi-36"),
            Selection::Part {
                corpus: "Public BI",
                name: "36 table subset",
            }
        );
    }

    #[test]
    fn the_full_set_is_read_off_the_name() {
        assert_eq!(
            Selection::of("public-bi"),
            Selection::Whole {
                corpus: "Public BI",
                members: "all 206 tables",
            }
        );
    }

    #[test]
    fn the_bare_name_is_a_prefix_of_the_subset_and_is_not_taken_for_it() {
        // `public-bi` starts `public-bi-36`, so reading the prefix first would make every subset
        // number look like a full set number, which is the exact confusion this module is for.
        assert_ne!(Selection::of("public-bi"), Selection::of("public-bi-36"));
    }

    #[test]
    fn spelling_and_punctuation_do_not_change_what_it_is() {
        assert_eq!(Selection::of("PublicBI_36"), Selection::of("public-bi-36"));
        assert_eq!(Selection::of("public_bi"), Selection::of("public-bi"));
    }

    #[test]
    fn a_corpus_with_no_parts_is_left_alone() {
        assert_eq!(Selection::of("clickbench-hits"), Selection::Undivided);
        assert_eq!(Selection::of("tpch-sf1"), Selection::Undivided);
        assert!(Selection::of("clickbench-hits").label().is_empty());
    }

    #[test]
    fn both_parts_of_one_corpus_name_the_same_corpus() {
        assert_eq!(
            Selection::of("public-bi").corpus(),
            Selection::of("public-bi-36").corpus()
        );
        assert_eq!(Selection::of("silesia").corpus(), None);
    }

    #[test]
    fn neither_part_has_a_name_that_is_only_the_benchmark() {
        // The point of the type. There is no way to print a Public BI number that does not say
        // which Public BI it is.
        for workload in ["public-bi", "public-bi-36"] {
            let label = Selection::of(workload).label();
            assert!(!label.is_empty(), "{workload} has no part in its name");
            assert_ne!(label, "Public BI");
        }
    }
}
