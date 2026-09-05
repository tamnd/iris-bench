//! The environment hash carried by every result row.

use bench_core::{EnvironmentHash, ParseEnvironmentHashError};

/// A digest that is not any real machine, written out so the tests read as text rather than as a
/// loop that builds the same bytes the code under test would.
const SAMPLE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[test]
fn a_hash_prints_as_the_string_it_parsed_from() {
    let hash: EnvironmentHash = SAMPLE.parse().expect("64 hex characters");
    assert_eq!(hash.to_string(), SAMPLE);
}

#[test]
fn a_hash_prints_every_byte_including_the_leading_zeroes() {
    // The obvious formatting mistake here loses a leading zero on any byte under sixteen, which
    // produces a shorter string that still looks like a hash and no longer round trips.
    let hash = EnvironmentHash::from_bytes([1_u8; 32]);
    assert_eq!(hash.to_string(), "01".repeat(32));
}

#[test]
fn a_hash_serialises_as_a_string_rather_than_as_a_list_of_numbers() {
    let hash: EnvironmentHash = SAMPLE.parse().expect("64 hex characters");
    let json = serde_json::to_string(&hash).expect("a hash serialises");

    assert_eq!(json, format!("\"{SAMPLE}\""));

    let back: EnvironmentHash = serde_json::from_str(&json).expect("and comes back");
    assert_eq!(back, hash);
}

#[test]
fn upper_case_hex_parses_and_then_prints_as_lower_case() {
    let hash: EnvironmentHash = SAMPLE.to_uppercase().parse().expect("64 hex characters");
    assert_eq!(hash.to_string(), SAMPLE);
}

#[test]
fn a_truncated_hash_is_rejected_rather_than_padded() {
    let error = SAMPLE[..63]
        .parse::<EnvironmentHash>()
        .expect_err("63 characters is not a hash");
    assert_eq!(error, ParseEnvironmentHashError::Length { found: 63 });
}

#[test]
fn something_that_is_not_hex_is_rejected_with_the_character_that_was_wrong() {
    let mut text = SAMPLE.to_owned();
    text.replace_range(10..11, "z");

    let error = text
        .parse::<EnvironmentHash>()
        .expect_err("z is not a hex digit");
    assert_eq!(error, ParseEnvironmentHashError::NotHex { found: 'z' });
}

#[test]
fn two_different_environments_do_not_compare_equal() {
    let one = EnvironmentHash::from_bytes([0_u8; 32]);
    let other = EnvironmentHash::from_bytes([1_u8; 32]);
    assert_ne!(one, other);
}
