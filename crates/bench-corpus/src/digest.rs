//! The content address every corpus file is pinned by.
//!
//! BLAKE3, because it is fast enough that hashing a hundred gigabyte corpus is not a reason to skip
//! the check, and skipping the check is the only way this ever goes wrong. A digest that is only
//! computed when somebody remembers to is not a pin.

use std::{fmt, fs::File, io, path::Path, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

/// How many bytes a BLAKE3 digest is.
const LENGTH: usize = 32;

/// A BLAKE3 digest of a file or a run of bytes.
///
/// Stored as bytes rather than as the hex string, so that two digests compare in one instruction
/// and a manifest cannot smuggle in an upper case spelling of the same value that fails an equality
/// check somewhere else.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Digest([u8; LENGTH]);

impl Digest {
    /// The digest of a run of bytes.
    #[must_use]
    pub fn of_bytes(bytes: &[u8]) -> Self {
        Self(*blake3::hash(bytes).as_bytes())
    }

    /// The digest of a file, read in chunks rather than loaded.
    ///
    /// Corpus files here run to tens of gigabytes, so reading one into memory to hash it would put
    /// a limit on corpus size that has nothing to do with anything.
    ///
    /// # Errors
    ///
    /// If the file cannot be opened or read.
    pub fn of_file(path: &Path) -> Result<Self, io::Error> {
        let mut hasher = blake3::Hasher::new();
        hasher.update_reader(File::open(path)?)?;
        Ok(Self(*hasher.finalize().as_bytes()))
    }

    /// A digest from bytes that have already been hashed somewhere else.
    ///
    /// For the streaming case, where the hasher is fed the same bytes that are being written and
    /// there is never a complete copy to hand to [`Self::of_bytes`].
    #[must_use]
    pub const fn from_bytes(bytes: [u8; LENGTH]) -> Self {
        Self(bytes)
    }

    /// The digest as its bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; LENGTH] {
        &self.0
    }

    /// The digest as lower case hex, which is the only spelling written anywhere.
    #[must_use]
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

/// Why a string is not a digest.
#[derive(Debug, thiserror::Error)]
pub enum DigestError {
    /// The string is not the length a hex BLAKE3 digest is.
    #[error("a digest is {expected} hex characters and this is {found}")]
    Length {
        /// How many characters a digest is.
        expected: usize,
        /// How many the string had.
        found: usize,
    },
    /// The string is the right length but is not hex.
    #[error("a digest is lower case hex and this is not: {0}")]
    NotHex(String),
    /// The string is hex but not all of it is lower case.
    ///
    /// Refused rather than accepted and normalised, because a manifest is read by people as well as
    /// by this code and two spellings of one digest is how a mismatch gets argued about.
    #[error("a digest is written in lower case hex and this is not: {0}")]
    NotLowerCase(String),
}

impl FromStr for Digest {
    type Err = DigestError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.len() != LENGTH * 2 {
            return Err(DigestError::Length {
                expected: LENGTH * 2,
                found: text.len(),
            });
        }
        if text.chars().any(|c| c.is_ascii_uppercase()) {
            return Err(DigestError::NotLowerCase(text.to_owned()));
        }
        let mut bytes = [0_u8; LENGTH];
        hex::decode_to_slice(text, &mut bytes).map_err(|_| DigestError::NotHex(text.to_owned()))?;
        Ok(Self(bytes))
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Debug for Digest {
    /// The hex rather than thirty two numbers, because a digest in a test failure is only useful if
    /// it can be compared against the one in the manifest by eye.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Digest({})", self.to_hex())
    }
}

impl Serialize for Digest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The published BLAKE3 digest of the empty input. If this ever changes, the pin on every
    /// corpus in the tree has silently moved and nothing else here would notice.
    const EMPTY: &str = "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262";

    #[test]
    fn the_empty_digest_is_the_published_one() {
        assert_eq!(Digest::of_bytes(b"").to_hex(), EMPTY);
    }

    #[test]
    fn a_digest_survives_hex_and_back() {
        let digest = Digest::of_bytes(b"iris-bench");
        assert_eq!(digest.to_hex().parse::<Digest>().unwrap(), digest);
    }

    #[test]
    fn upper_case_hex_is_refused_rather_than_normalised() {
        let error = EMPTY.to_uppercase().parse::<Digest>().unwrap_err();
        assert!(matches!(error, DigestError::NotLowerCase(_)));
    }

    #[test]
    fn a_digest_of_the_wrong_length_says_what_length_it_was() {
        let error = "abcd".parse::<Digest>().unwrap_err();
        assert!(format!("{error}").contains("this is 4"));
    }

    #[test]
    fn something_the_right_length_that_is_not_hex_is_refused() {
        let error = "z".repeat(64).parse::<Digest>().unwrap_err();
        assert!(matches!(error, DigestError::NotHex(_)));
    }

    #[test]
    fn a_file_and_its_bytes_hash_the_same() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("corpus");
        std::fs::write(&path, b"the same bytes either way").unwrap();
        assert_eq!(
            Digest::of_file(&path).unwrap(),
            Digest::of_bytes(b"the same bytes either way")
        );
    }
}
