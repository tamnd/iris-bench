//! What a query produced, in a form two different systems can be compared on.
//!
//! A timing is only worth having if the two systems being timed answered the same question. The
//! motivating example in the fair benchmarking literature is a prototype that was faster because it
//! used hardcoded group counts, integer types too small to hold the aggregate, and unhandled
//! overflow. It was, of course, faster. So every query here produces a digest of its canonicalised
//! result and the digests are compared across systems, and a row whose result does not match is
//! marked incorrect and excluded from every aggregate.
//!
//! Canonicalised is the load bearing word, and it is why this module exists rather than each driver
//! rendering its own results. Three systems asked the same question return the same answer in three
//! different shapes: different float formatting, different null spellings, different row order for a
//! query that never specified one. None of those is a wrong answer and all of them are a different
//! digest.
//!
//! # The two decisions with a real cost
//!
//! **Floats are rendered to six significant digits.** Two engines summing a hundred million values
//! will not agree past that, because a parallel partial sum and a serial one add the same numbers in
//! a different order. Six digits survives that and still catches the class of thing this check
//! exists for, since a hardcoded group count or an overflowed accumulator is not a seventh digit
//! difference. What it does not catch is a genuine small error, and that is the price. A digest
//! cannot express a tolerance, so the tolerance has to be in the canonical form or nowhere.
//!
//! **Rows are sorted unless the query asked for an order.** A query with no `ORDER BY` has no
//! defined row order, so comparing two engines on the order they happened to produce is comparing
//! them on something neither was asked to do. When the query does specify an order, the rows are
//! left alone and the order is part of what is checked.

use std::fmt::Write as _;

/// How many significant decimal digits a float is rendered to.
///
/// See the module documentation. This is the number that decides whether two engines that computed
/// the same sum in a different order agree.
///
/// Six is a conservative starting point rather than a measured one. The milestone issue that
/// compares digests across all three systems is where it gets calibrated against what three engines
/// actually return on real queries, which is a better basis than an estimate of how much a parallel
/// summation drifts. Until then it errs towards agreeing, and the cost of that is in the module
/// documentation.
const FLOAT_DIGITS: usize = 6;

/// What separates two values on a row.
const FIELD: char = '\t';

/// One value in a result row.
///
/// Deliberately small. This is not a type system for query results, it is the set of shapes a
/// `ClickBench` or TPC-H answer comes back in, and every one of them has an unambiguous rendering.
#[derive(Clone, PartialEq, Debug)]
pub enum Value {
    /// No value. Rendered as `\N`, which is what a text of "\N" escapes away from.
    Null,
    /// A boolean, rendered as `true` or `false`.
    Bool(bool),
    /// A signed integer, rendered in decimal. Unsigned results widen into this, because no result
    /// in any workload here exceeds an `i64` and a type that could hold more would need a rendering
    /// that says which type it was.
    Int(i64),
    /// A floating point number, rendered to six significant digits.
    Float(f64),
    /// Text, rendered with backslash, tab and newline escaped.
    Text(String),
    /// A calendar date, as days since 1970-01-01, rendered `YYYY-MM-DD`.
    Date(i32),
    /// A time of day, as microseconds since midnight, rendered `HH:MM:SS` with a fraction only when
    /// there is one.
    Time(i64),
    /// An instant, as microseconds since 1970-01-01 00:00:00, rendered `YYYY-MM-DD HH:MM:SS` with a
    /// fraction only when there is one.
    ///
    /// No time zone, because none of these workloads has one. A system that returns a zoned value
    /// has to say what it converted to, and that belongs in its `CONFIG.md` as a deviation rather
    /// than in a canonical form that pretends the question did not arise.
    Timestamp(i64),
}

impl Value {
    /// Writes this value in the canonical form.
    fn render(&self, into: &mut String) {
        match self {
            Self::Null => into.push_str("\\N"),
            Self::Bool(value) => into.push_str(if *value { "true" } else { "false" }),
            Self::Int(value) => {
                let _ = write!(into, "{value}");
            }
            Self::Float(value) => into.push_str(&float(*value)),
            Self::Text(value) => {
                for character in value.chars() {
                    match character {
                        '\\' => into.push_str("\\\\"),
                        '\t' => into.push_str("\\t"),
                        '\n' => into.push_str("\\n"),
                        '\r' => into.push_str("\\r"),
                        other => into.push(other),
                    }
                }
            }
            Self::Date(value) => into.push_str(&date(i64::from(*value))),
            Self::Time(value) => into.push_str(&clock(*value)),
            Self::Timestamp(value) => {
                let days = value.div_euclid(MICROSECONDS_PER_DAY);
                let rest = value.rem_euclid(MICROSECONDS_PER_DAY);
                let _ = write!(into, "{} {}", date(days), clock(rest));
            }
        }
    }
}

/// Microseconds in a day, which is the unit both the time and the timestamp forms are counted in.
const MICROSECONDS_PER_DAY: i64 = 24 * 60 * 60 * 1_000_000;

/// A day number as `YYYY-MM-DD`.
fn date(days: i64) -> String {
    let (year, month, day) = civil(days);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Microseconds since midnight as `HH:MM:SS`, with a fraction only when there is one.
///
/// The fraction is trimmed rather than padded, so a system that stores milliseconds and one that
/// stores microseconds render the same instant the same way. Padding would make them disagree about
/// a difference that is in their storage rather than in their answer.
fn clock(microseconds: i64) -> String {
    let microseconds = microseconds.rem_euclid(MICROSECONDS_PER_DAY);
    let seconds = microseconds / 1_000_000;
    let fraction = microseconds % 1_000_000;
    let mut text = format!(
        "{:02}:{:02}:{:02}",
        seconds / 3_600,
        (seconds / 60) % 60,
        seconds % 60
    );
    if fraction != 0 {
        let digits = format!("{fraction:06}");
        let _ = write!(text, ".{}", digits.trim_end_matches('0'));
    }
    text
}

/// The calendar date a day number falls on.
///
/// Howard Hinnant's `civil_from_days`, which is the standard shift of the year to start in March so
/// that the leap day lands at the end of it and the month lengths become a single linear formula.
/// Written out rather than pulled in from a date library, because this crate is what every other
/// crate here depends on and a canonical rendering is not worth a dependency tree.
fn civil(days: i64) -> (i64, i64, i64) {
    // 719468 is the number of days from 0000-03-01 to 1970-01-01, which moves the epoch to the
    // start of the shifted year.
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// A float in the canonical form.
///
/// Not a straight `{:.6e}`, because that renders every integral value in exponent form and a result
/// table full of `1.000000e0` is unreadable by the person who has to work out why two systems
/// disagree. Values that round to something a person would write plainly are written plainly.
fn float(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-Infinity".to_owned()
        } else {
            "Infinity".to_owned()
        };
    }
    // Negative zero and zero are the same number and two engines will disagree about which one a
    // sum of nothing produced.
    if value == 0.0 {
        return "0".to_owned();
    }

    let magnitude = value.abs();
    if !(1e-4..1e15).contains(&magnitude) {
        return exponent(value);
    }

    let rounded = round_to_significant(value, FLOAT_DIGITS);
    // Enough places after the point to carry six significant digits at this magnitude, then the
    // trailing zeros the rounding produced are taken off so that 1.5 and 1.500000 are one string.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the magnitude is bounded above, so the exponent is a small integer"
    )]
    let places = FLOAT_DIGITS.saturating_sub(magnitude.log10().floor() as usize + 1);
    let mut text = format!("{rounded:.places$}");
    if text.contains('.') {
        text = text.trim_end_matches('0').trim_end_matches('.').to_owned();
    }
    text
}

/// A very large or very small number, in exponent form.
///
/// Formatted straight to six significant digits rather than rounded first and then printed. At the
/// far ends of the range the scaling factor that rounding needs is itself not representable, so a
/// value that came back from rounding would carry a tail of digits that are an artefact of the
/// rounding rather than anything the query computed.
fn exponent(value: f64) -> String {
    let text = format!("{:.*e}", FLOAT_DIGITS - 1, value);
    let (mantissa, power) = text.split_once('e').unwrap_or((text.as_str(), "0"));
    let mantissa = if mantissa.contains('.') {
        mantissa.trim_end_matches('0').trim_end_matches('.')
    } else {
        mantissa
    };
    format!("{mantissa}e{power}")
}

/// Rounds to `digits` significant decimal digits.
fn round_to_significant(value: f64, digits: usize) -> f64 {
    if value == 0.0 || !value.is_finite() {
        return value;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        reason = "digits is six, and a decimal exponent of a finite f64 fits in an i32 with room"
    )]
    let exponent = digits as i32 - 1 - value.abs().log10().floor() as i32;
    let scale = 10_f64.powi(exponent);
    // A scale that overflowed or underflowed would turn the value into an infinity or a zero, which
    // is a worse answer than not rounding it.
    if scale.is_finite() && scale != 0.0 {
        let scaled = value * scale;
        if scaled.is_finite() {
            return scaled.round() / scale;
        }
    }
    value
}

/// A result being assembled, one row at a time.
///
/// Built rather than constructed, because the row count and the canonical body have to agree and a
/// driver that reports one and produces the other is the failure this whole module exists to catch.
#[derive(Clone, Debug)]
pub struct Rows {
    /// How many columns the first row had, which every later row has to match.
    columns: Option<usize>,
    /// Each row already rendered, kept separately so they can be sorted.
    rows: Vec<String>,
}

impl Rows {
    /// Starts an empty result.
    #[must_use]
    pub fn new() -> Self {
        Self {
            columns: None,
            rows: Vec::new(),
        }
    }

    /// Adds one row.
    ///
    /// # Errors
    ///
    /// If it does not have the same number of columns as the rows before it. A result whose rows
    /// have different widths is not a table, and a driver that produced one has a bug that would
    /// otherwise show up as a digest mismatch against every other system at once.
    pub fn push(&mut self, values: impl IntoIterator<Item = Value>) -> Result<(), WidthError> {
        let values: Vec<Value> = values.into_iter().collect();
        match self.columns {
            None => self.columns = Some(values.len()),
            Some(columns) if columns != values.len() => {
                return Err(WidthError {
                    row: self.rows.len(),
                    wanted: columns,
                    found: values.len(),
                });
            }
            Some(_) => {}
        }

        let mut line = String::new();
        for (index, value) in values.iter().enumerate() {
            if index > 0 {
                line.push(FIELD);
            }
            value.render(&mut line);
        }
        self.rows.push(line);
        Ok(())
    }

    /// Finishes the result.
    ///
    /// `ordered` says whether the query specified a row order. When it did not, the rows are sorted
    /// here, so that two engines are not compared on something neither of them was asked to do.
    #[must_use]
    pub fn finish(mut self, ordered: bool) -> Answer {
        if !ordered {
            self.rows.sort_unstable();
        }
        let mut body = self.rows.join("\n");
        if !body.is_empty() {
            body.push('\n');
        }
        Answer {
            rows: self.rows.len() as u64,
            columns: self.columns.unwrap_or(0),
            body,
        }
    }
}

impl Default for Rows {
    fn default() -> Self {
        Self::new()
    }
}

/// A row that does not have the same number of columns as the ones before it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
#[error("row {row} has {found} columns and the rows before it have {wanted}")]
pub struct WidthError {
    /// Which row, counting from zero.
    pub row: usize,
    /// How many columns the result has.
    pub wanted: usize,
    /// How many this row had.
    pub found: usize,
}

/// What a query produced, canonicalised.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Answer {
    /// How many rows came back.
    pub rows: u64,
    /// How many columns each row has, or zero for a result with no rows.
    pub columns: usize,
    /// The canonical rendering, one row per line, values separated by a tab.
    ///
    /// This is what gets digested and compared across systems. It is a `String` rather than a
    /// digest because the digesting is not the driver's job: a driver that hashes its own results
    /// is a driver that can agree with itself about a wrong answer.
    pub body: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(value: Value) -> String {
        let mut rows = Rows::new();
        rows.push([value]).unwrap();
        rows.finish(true).body
    }

    #[test]
    fn a_float_is_rendered_the_way_a_person_would_write_it() {
        assert_eq!(one(Value::Float(1.5)), "1.5\n");
        assert_eq!(one(Value::Float(1.0)), "1\n");
        assert_eq!(one(Value::Float(-2.25)), "-2.25\n");
        assert_eq!(one(Value::Float(1234.5)), "1234.5\n");
    }

    #[test]
    fn two_engines_that_summed_in_a_different_order_agree() {
        // The seventh digit apart, which is what a parallel partial sum and a serial one differ by
        // on a hundred million values, and not the kind of difference this check exists to catch.
        assert_eq!(
            one(Value::Float(1_234_567.0)),
            one(Value::Float(1_234_568.0))
        );
        // The sixth, which is.
        assert_ne!(
            one(Value::Float(1_234_500.0)),
            one(Value::Float(1_234_600.0))
        );
    }

    #[test]
    fn zero_has_one_spelling() {
        assert_eq!(one(Value::Float(0.0)), "0\n");
        assert_eq!(one(Value::Float(-0.0)), "0\n");
    }

    #[test]
    fn a_number_too_large_or_too_small_to_write_plainly_gets_an_exponent() {
        assert_eq!(one(Value::Float(1.5e300)), "1.5e300\n");
        assert_eq!(one(Value::Float(2.5e-9)), "2.5e-9\n");
    }

    #[test]
    fn what_is_not_a_number_says_so_rather_than_being_dropped() {
        assert_eq!(one(Value::Float(f64::NAN)), "NaN\n");
        assert_eq!(one(Value::Float(f64::INFINITY)), "Infinity\n");
        assert_eq!(one(Value::Float(f64::NEG_INFINITY)), "-Infinity\n");
    }

    #[test]
    fn text_that_contains_the_separators_is_escaped() {
        assert_eq!(one(Value::Text("a\tb".to_owned())), "a\\tb\n");
        assert_eq!(one(Value::Text("a\nb".to_owned())), "a\\nb\n");
        assert_eq!(one(Value::Text("a\\b".to_owned())), "a\\\\b\n");
    }

    #[test]
    fn text_that_spells_a_null_is_not_a_null() {
        // Without the backslash escape these two would digest the same, and a driver returning the
        // literal text would be indistinguishable from one returning nothing.
        assert_ne!(one(Value::Text("\\N".to_owned())), one(Value::Null));
    }

    #[test]
    fn a_date_is_rendered_as_a_calendar_date() {
        assert_eq!(one(Value::Date(0)), "1970-01-01\n");
        assert_eq!(one(Value::Date(19_723)), "2024-01-01\n");
        // The leap day, which is the whole reason the calendar arithmetic is not a division.
        assert_eq!(one(Value::Date(19_782)), "2024-02-29\n");
        // 1900 is not a leap year and 2000 is, which is the century rule and the exception to it.
        assert_eq!(one(Value::Date(-25_567)), "1900-01-01\n");
        assert_eq!(one(Value::Date(11_016)), "2000-02-29\n");
    }

    #[test]
    fn a_date_before_the_epoch_counts_backwards_rather_than_wrapping() {
        assert_eq!(one(Value::Date(-1)), "1969-12-31\n");
        assert_eq!(one(Value::Date(-365)), "1969-01-01\n");
    }

    #[test]
    fn a_time_shows_a_fraction_only_when_there_is_one() {
        assert_eq!(one(Value::Time(0)), "00:00:00\n");
        assert_eq!(one(Value::Time(3_661_000_000)), "01:01:01\n");
        assert_eq!(one(Value::Time(1_500_000)), "00:00:01.5\n");
        assert_eq!(one(Value::Time(1_000_001)), "00:00:01.000001\n");
    }

    #[test]
    fn a_system_that_stores_milliseconds_agrees_with_one_that_stores_microseconds() {
        // The same instant, arrived at from two different storage precisions. Padding the fraction
        // to a fixed width would make these disagree about something that is not in the answer.
        assert_eq!(one(Value::Time(1_500_000)), one(Value::Time(1_500_000)));
        assert_eq!(one(Value::Time(2_000_000)), "00:00:02\n");
    }

    #[test]
    fn a_timestamp_is_a_date_and_a_time() {
        assert_eq!(one(Value::Timestamp(0)), "1970-01-01 00:00:00\n");
        assert_eq!(
            one(Value::Timestamp(1_704_067_199_000_000)),
            "2023-12-31 23:59:59\n"
        );
        assert_eq!(one(Value::Timestamp(-1)), "1969-12-31 23:59:59.999999\n");
    }

    #[test]
    fn a_query_that_asked_for_no_order_is_not_compared_on_one() {
        let mut first = Rows::new();
        first.push([Value::Int(2)]).unwrap();
        first.push([Value::Int(1)]).unwrap();
        let mut second = Rows::new();
        second.push([Value::Int(1)]).unwrap();
        second.push([Value::Int(2)]).unwrap();

        assert_eq!(first.clone().finish(false), second.clone().finish(false));
        assert_ne!(first.finish(true), second.finish(true));
    }

    #[test]
    fn a_row_of_a_different_width_is_refused_rather_than_rendered() {
        let mut rows = Rows::new();
        rows.push([Value::Int(1), Value::Int(2)]).unwrap();
        let error = rows.push([Value::Int(3)]).unwrap_err();
        assert_eq!(error.wanted, 2);
        assert_eq!(error.found, 1);
    }

    #[test]
    fn an_empty_result_is_still_an_answer() {
        let answer = Rows::new().finish(true);
        assert_eq!(answer.rows, 0);
        assert_eq!(answer.columns, 0);
        assert!(answer.body.is_empty());
    }

    #[test]
    fn the_row_count_and_the_body_cannot_disagree() {
        let mut rows = Rows::new();
        for value in 0..3 {
            rows.push([Value::Int(value)]).unwrap();
        }
        let answer = rows.finish(true);
        assert_eq!(answer.rows, 3);
        assert_eq!(answer.body.lines().count(), 3);
    }
}
