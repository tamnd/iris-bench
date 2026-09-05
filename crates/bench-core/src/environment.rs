//! The identity of the conditions a measurement was taken under.
//!
//! `bench-env` works out what a machine is and how it is configured and reduces that to one digest.
//! The digest lives here rather than there because it belongs to the row. Every result carries one,
//! and two rows with different digests were not taken under the same conditions, so a reader
//! comparing two numbers checks one field instead of remembering what was true on the day.
//!
//! # What is in it and what is not
//!
//! Only what the machine is and how it is set up. The processor, how much memory is fitted, the
//! kernel, the frequency governor, whether turbo is on, whether the run was pinned, and whether
//! there is a hypervisor underneath.
//!
//! Not what the machine was doing at the instant. Free memory and load average move between one
//! measurement and the next, so hashing them would give every row a digest of its own and the field
//! would stop being able to group anything. Those observations are what the eligibility gates read,
//! and a gate that fails stops the run outright, which is a stronger protection than a digest. They
//! are recorded beside the digest rather than inside it.
//!
//! # Why unobservable is not the same as absent
//!
//! A setting that cannot be read on a platform is recorded as unreadable rather than skipped, and
//! that recording is inside the digest. So the same machine measured under two operating systems
//! produces two different digests, and a row from one cannot be quietly compared against a row from
//! the other. That is the intended behaviour: the comparison is not wrong because the numbers are
//! wrong, it is wrong because nobody checked the same things in both cases.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A digest of the machine and its configuration, carried by every result row.
///
/// Compared with `==` and nothing else. There is no ordering on it and no notion of one environment
/// being close to another, because two environments are either the same one or they are not.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct EnvironmentHash([u8; 32]);

impl EnvironmentHash {
    /// Wraps 32 bytes that some hash function has already produced.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The bytes, for a caller writing them into a binary column.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Lower case hex, all 64 characters of it.
///
/// Not truncated. A short digest reads nicely in a table and collides eventually, and the whole
/// value of this field is that two rows that share it really were taken under the same conditions.
impl fmt::Display for EnvironmentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Prints as the hex rather than as a list of numbers, because a digest in a debug log that a reader
/// cannot compare against a result row by eye is not doing its job.
impl fmt::Debug for EnvironmentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EnvironmentHash({self})")
    }
}

/// What is wrong with a string that was supposed to be an environment hash.
#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ParseEnvironmentHashError {
    /// The string was not 64 characters long.
    #[error("an environment hash is 64 hex characters, this one is {found}")]
    Length {
        /// How many characters were there.
        found: usize,
    },
    /// The string was the right length but held something that is not hex.
    #[error("an environment hash is hex and this one contains {found:?}")]
    NotHex {
        /// The first character that is not a hex digit.
        found: char,
    },
}

impl FromStr for EnvironmentHash {
    type Err = ParseEnvironmentHashError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.len() != 64 {
            return Err(ParseEnvironmentHashError::Length { found: text.len() });
        }
        if let Some(found) = text.chars().find(|c| !c.is_ascii_hexdigit()) {
            return Err(ParseEnvironmentHashError::NotHex { found });
        }

        let (pairs, _) = text.as_bytes().as_chunks::<2>();
        let mut out = [0_u8; 32];
        for (slot, pair) in out.iter_mut().zip(pairs) {
            // Both characters passed `is_ascii_hexdigit` above, so neither conversion can fail.
            let hi = char::from(pair[0]).to_digit(16).unwrap_or(0);
            let lo = char::from(pair[1]).to_digit(16).unwrap_or(0);
            *slot = u8::try_from(hi * 16 + lo).unwrap_or(0);
        }
        Ok(Self(out))
    }
}

/// Serialised as the hex string, not as an array of 32 numbers.
///
/// The digest ends up in JSON that a person reads and in a Parquet column that a person queries, and
/// in both places it has to be the same string they would paste from a result table.
impl Serialize for EnvironmentHash {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for EnvironmentHash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}
