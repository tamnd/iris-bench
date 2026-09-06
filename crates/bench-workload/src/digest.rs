//! What an answer hashes to.

use std::fmt;

use bench_driver::Answer;

/// The `BLAKE3` digest of one answer.
///
/// Taken here rather than inside a driver, for the reason `bench_driver::answer` gives: a driver
/// that hashes its own output is a driver that can agree with itself about a wrong answer. The
/// driver hands back a canonical rendering and somebody else decides what it hashes to.
///
/// The row count and the column count go into the hash along with the body, both as fixed width
/// little endian, so that the three parts cannot be rearranged into each other and so that two
/// systems which produced the same characters out of a differently shaped result are not recorded
/// as agreeing. A result with no rows has an empty body and is exactly the case where the shape is
/// the only thing left to compare.
#[derive(Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct Digest([u8; 32]);

impl Digest {
    /// Hashes one answer.
    #[must_use]
    pub fn of(answer: &Answer) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&answer.rows.to_le_bytes());
        hasher.update(
            &u64::try_from(answer.columns)
                .unwrap_or(u64::MAX)
                .to_le_bytes(),
        );
        hasher.update(answer.body.as_bytes());
        Self(*hasher.finalize().as_bytes())
    }

    /// The thirty two bytes.
    #[must_use]
    pub fn bytes(&self) -> [u8; 32] {
        self.0
    }

    /// The first sixteen hex characters, for a table a person has to read across.
    ///
    /// Never for comparing. Two digests are compared as digests, and a short form that could
    /// collide is a short form somebody will eventually compare anyway if it is easy to.
    #[must_use]
    pub fn short(&self) -> String {
        hex::encode(&self.0[..8])
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self.0))
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Digest({self})")
    }
}

impl From<Digest> for String {
    fn from(digest: Digest) -> Self {
        digest.to_string()
    }
}

impl TryFrom<String> for Digest {
    type Error = ParseDigestError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.parse()
    }
}

impl std::str::FromStr for Digest {
    type Err = ParseDigestError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        // Lower case only, the same rule the corpus manifests are held to. Two spellings of one
        // value is how a mismatch ends up being argued about instead of investigated.
        if text.len() != 64
            || text
                .chars()
                .any(|c| !c.is_ascii_digit() && !('a'..='f').contains(&c))
        {
            return Err(ParseDigestError);
        }
        let mut bytes = [0u8; 32];
        hex::decode_to_slice(text, &mut bytes).map_err(|_| ParseDigestError)?;
        Ok(Self(bytes))
    }
}

/// A string that is not sixty four lower case hex characters.
#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
#[error("a digest is sixty four lower case hex characters")]
pub struct ParseDigestError;

#[cfg(test)]
mod tests {
    use bench_driver::{Rows, Value};

    use super::*;

    fn answer(values: &[Value], ordered: bool) -> Answer {
        let mut rows = Rows::new();
        for value in values {
            rows.push([value.clone()]).unwrap();
        }
        rows.finish(ordered)
    }

    #[test]
    fn the_same_answer_hashes_the_same_way_twice() {
        let one = Digest::of(&answer(&[Value::Int(1), Value::Int(2)], true));
        let two = Digest::of(&answer(&[Value::Int(1), Value::Int(2)], true));
        assert_eq!(one, two);
    }

    #[test]
    fn two_engines_that_returned_the_rows_in_a_different_order_agree() {
        // The canonical form sorts an unordered result before it is rendered, so this is really a
        // test that the digest is taken over that form rather than over what a driver happened to
        // hand back first.
        let one = Digest::of(&answer(&[Value::Int(1), Value::Int(2)], false));
        let two = Digest::of(&answer(&[Value::Int(2), Value::Int(1)], false));
        assert_eq!(one, two);
    }

    #[test]
    fn a_query_that_promised_an_order_and_returned_another_one_disagrees() {
        let one = Digest::of(&answer(&[Value::Int(1), Value::Int(2)], true));
        let two = Digest::of(&answer(&[Value::Int(2), Value::Int(1)], true));
        assert_ne!(one, two);
    }

    #[test]
    fn two_empty_results_of_different_widths_do_not_agree() {
        // Both render as nothing at all, so the body on its own would call them the same answer.
        let narrow = Answer {
            rows: 0,
            columns: 1,
            body: String::new(),
        };
        let wide = Answer {
            rows: 0,
            columns: 2,
            body: String::new(),
        };
        assert_ne!(Digest::of(&narrow), Digest::of(&wide));
    }

    #[test]
    fn a_digest_reads_back_as_the_digest_it_printed() {
        let digest = Digest::of(&answer(&[Value::Int(7)], true));
        let printed = digest.to_string();
        assert_eq!(printed.len(), 64);
        assert_eq!(printed.parse::<Digest>().unwrap(), digest);
        assert!(printed.starts_with(&digest.short()));
    }

    #[test]
    fn an_upper_case_spelling_is_refused_rather_than_normalised() {
        let printed = Digest::of(&answer(&[Value::Int(7)], true)).to_string();
        assert!(printed.to_uppercase().parse::<Digest>().is_err());
        assert!("".parse::<Digest>().is_err());
        assert!("zz".repeat(32).parse::<Digest>().is_err());
    }
}
