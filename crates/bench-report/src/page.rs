//! A page of numbers, and the notices it is not allowed to be rendered without.
//!
//! The rule this shape exists for is that the trademark attribution appears on every page carrying
//! a TPC number. Written as a rule, somebody has to remember it on every page. Written as a
//! [`Page`] that collects its notices from the rows it was given, there is no page that carries one
//! of those numbers and does not carry the notice, because the same call produces both.
//!
//! What a value is here is a string. The measurement types, the intervals and the comparison rules
//! belong to the milestone that owns this crate. What had to exist first is the part that decides
//! what a number may be called, because that is the part where getting it wrong is not a rendering
//! bug.

use std::fmt::Write as _;

use crate::tpc::{self, Cited, Family, WordingError};

/// One measurement on a page.
#[derive(Clone, Debug)]
pub struct Row {
    /// Which workload, as the identifier the corpus and the result files use.
    pub workload: String,
    /// Which system produced the number.
    pub system: String,
    /// The number, already rendered.
    pub value: String,
    /// Which family the workload belongs to, worked out from its name rather than declared.
    family: Family,
    /// Where the number came from, when it is not one of ours.
    origin: Option<String>,
}

impl Row {
    /// A number this harness measured.
    ///
    /// # Errors
    ///
    /// If the workload names a TPC family and a skew, which is a different benchmark and needs a
    /// different name.
    pub fn measured(
        workload: impl Into<String>,
        system: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, WordingError> {
        let workload = workload.into();
        let family = tpc::check(&workload)?;
        Ok(Self {
            workload,
            system: system.into(),
            value: value.into(),
            family,
            origin: None,
        })
    }

    /// A number somebody else published.
    ///
    /// # Errors
    ///
    /// If the workload names a TPC family and a skew, or if what is being cited is an official TPC
    /// Result, which may not appear next to a number from here.
    pub fn cited(
        workload: impl Into<String>,
        system: impl Into<String>,
        value: impl Into<String>,
        cited: Cited,
    ) -> Result<Self, WordingError> {
        let workload = workload.into();
        let family = tpc::check(&workload)?;
        let origin = match cited {
            Cited::Published { origin } => origin,
            Cited::OfficialTpcResult { origin } => {
                return Err(WordingError::OfficialTpcResult { workload, origin });
            }
        };
        Ok(Self {
            workload,
            system: system.into(),
            value: value.into(),
            family,
            origin: Some(origin),
        })
    }

    /// What has to be said about this row next to the number rather than under the table.
    ///
    /// A scale factor the specification does not define is the case this exists for. A reader
    /// scanning a column of TPC-H numbers should be able to see which of them is at a size TPC
    /// never defined without going to a footnote, because a footnote is the part that does not get
    /// copied along with the number.
    #[must_use]
    pub fn note(&self) -> String {
        let mut parts = Vec::new();
        if let Some(scale) = tpc::scale_of(&self.workload)
            && !self.family.scale_is_compliant(scale)
        {
            parts.push(format!(
                "scale factor {scale} is not a compliant scale factor, so this is a deviation"
            ));
        }
        if let Some(origin) = self.origin.as_ref() {
            parts.push(format!("reported by {origin}, not measured here"));
        }
        parts.join("; ")
    }

    /// How this row's workload is named in output.
    #[must_use]
    pub fn label(&self) -> String {
        match self.family.label() {
            "" => self.workload.clone(),
            qualified => format!("{} ({qualified})", self.workload),
        }
    }
}

/// A table of rows and everything that has to be printed with them.
#[derive(Clone, Debug, Default)]
pub struct Page {
    /// What the table is about.
    pub title: String,
    /// The rows, in the order they were added.
    rows: Vec<Row>,
}

impl Page {
    /// An empty page.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            rows: Vec::new(),
        }
    }

    /// Adds a row.
    pub fn push(&mut self, row: Row) {
        self.rows.push(row);
    }

    /// Every notice this page's rows require, once each and in a stable order.
    #[must_use]
    pub fn notices(&self) -> Vec<&'static str> {
        let mut notices: Vec<&'static str> = Vec::new();
        for row in &self.rows {
            if let Some(notice) = row.family.notice()
                && !notices.contains(&notice)
            {
                notices.push(notice);
            }
        }
        notices
    }

    /// The page as text, with its notices under it.
    ///
    /// There is no way to get the table without the notices, which is the whole reason this returns
    /// a page rather than a table.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        if !self.title.is_empty() {
            out.push_str(&self.title);
            out.push_str("\n\n");
        }
        for row in &self.rows {
            let note = row.note();
            let _ = write!(out, "{}  {}  {}", row.label(), row.system, row.value);
            if !note.is_empty() {
                let _ = write!(out, "  [{note}]");
            }
            out.push('\n');
        }
        for notice in self.notices() {
            out.push('\n');
            out.push_str(notice);
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_carrying_a_tpc_number_carries_the_notice() {
        let mut page = Page::new("Scan rates");
        page.push(Row::measured("tpch-sf1", "iris", "1.2 GB/s").unwrap());
        let rendered = page.render();
        assert!(rendered.contains("derived from TPC-H"));
        assert!(rendered.contains("trademarks of the Transaction Processing Performance Council"));
    }

    #[test]
    fn a_page_of_nobody_elses_numbers_carries_nothing_extra() {
        let mut page = Page::new("Scan rates");
        page.push(Row::measured("clickbench-hits", "iris", "1.2 GB/s").unwrap());
        assert!(page.notices().is_empty());
        assert!(!page.render().contains("Transaction Processing"));
    }

    #[test]
    fn the_notice_appears_once_however_many_rows_need_it() {
        let mut page = Page::new("Scan rates");
        page.push(Row::measured("tpch-sf1", "iris", "1.2 GB/s").unwrap());
        page.push(Row::measured("tpch-sf20", "iris", "1.1 GB/s").unwrap());
        page.push(Row::measured("tpcds-sf1000", "iris", "0.9 GB/s").unwrap());
        assert_eq!(page.notices().len(), 1);
    }

    #[test]
    fn a_non_compliant_scale_factor_is_marked_in_the_row_and_not_in_a_footnote() {
        let row = Row::measured("tpch-sf20", "iris", "1.1 GB/s").unwrap();
        assert!(row.note().contains("not a compliant scale factor"));

        let mut page = Page::new("Scan rates");
        page.push(row);
        let rendered = page.render();
        let table = rendered
            .lines()
            .find(|line| line.contains("tpch-sf20"))
            .unwrap();
        assert!(table.contains("deviation"));
    }

    #[test]
    fn a_compliant_scale_factor_is_not_marked() {
        assert!(
            Row::measured("tpch-sf1", "iris", "1.2 GB/s")
                .unwrap()
                .note()
                .is_empty()
        );
    }

    #[test]
    fn an_official_tpc_result_cannot_be_put_on_a_page() {
        let error = Row::cited(
            "tpch-sf1000",
            "some appliance",
            "4 000 000 QphH",
            Cited::OfficialTpcResult {
                origin: "the TPC results list".to_owned(),
            },
        )
        .unwrap_err();
        assert!(matches!(error, WordingError::OfficialTpcResult { .. }));
    }

    #[test]
    fn a_number_from_somewhere_else_says_so_in_its_row() {
        let row = Row::cited(
            "clickbench-hits",
            "some engine",
            "12.3",
            Cited::Published {
                origin: "the ClickBench leaderboard".to_owned(),
            },
        )
        .unwrap();
        assert!(row.note().contains("not measured here"));
    }

    #[test]
    fn a_skewed_generator_cannot_be_filed_under_a_tpc_name() {
        assert!(Row::measured("tpch-skew-sf1", "iris", "1.2 GB/s").is_err());
    }
}
