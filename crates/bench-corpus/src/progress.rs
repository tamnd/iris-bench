//! Saying how far along something long is.
//!
//! A corpus is tens of gigabytes whether it arrives over a link or comes out of a generator, and
//! the two paths report the same way because they are the same wait from the outside. Nothing here
//! decides how a report is displayed, only when one is due.

use std::io::{self, Read};

/// How often to report, in bytes.
///
/// Small enough that a stalled transfer is visible within a few seconds on any link worth using,
/// large enough that the reporting is not itself measurable next to the hashing.
pub(crate) const REPORT_EVERY: u64 = 64 << 20;

/// How far along one file is.
#[derive(Clone, Copy, Debug)]
pub struct Progress<'a> {
    /// The path inside the corpus.
    pub path: &'a str,
    /// How many bytes have gone through.
    pub done: u64,
    /// How many the manifest says there are.
    pub total: u64,
}

/// A reader that counts what goes through it and says so every so often.
pub(crate) struct Counting<'a, R> {
    /// The reader the bytes are actually coming from.
    pub(crate) inner: R,
    /// How many have gone through.
    pub(crate) seen: u64,
    /// The count at which to report next.
    pub(crate) next: u64,
    /// Which file, for the report.
    pub(crate) path: &'a str,
    /// How many bytes the manifest says there are, for the report.
    pub(crate) total: u64,
    /// Who to tell.
    pub(crate) watch: &'a mut dyn FnMut(Progress<'_>),
}

impl<'a, R> Counting<'a, R> {
    /// Wraps a reader, with the first report due once `REPORT_EVERY` bytes have gone through.
    pub(crate) fn new(
        inner: R,
        path: &'a str,
        total: u64,
        watch: &'a mut dyn FnMut(Progress<'_>),
    ) -> Self {
        Self {
            inner,
            seen: 0,
            next: REPORT_EVERY,
            path,
            total,
            watch,
        }
    }
}

impl<R: Read> Read for Counting<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.seen += read as u64;
        if self.seen >= self.next || read == 0 {
            self.next = self.seen + REPORT_EVERY;
            (self.watch)(Progress {
                path: self.path,
                done: self.seen,
                total: self.total,
            });
        }
        Ok(read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_read_reports_once_at_the_end() {
        let mut seen = Vec::new();
        let mut watch = |progress: Progress<'_>| seen.push(progress.done);
        let mut counting = Counting::new(&b"twelve bytes"[..], "example", 12, &mut watch);
        let mut sink = Vec::new();
        counting.read_to_end(&mut sink).unwrap();
        // Two reads: one that gets everything and one that gets nothing and ends it. The report is
        // due on the second because a zero read is the end, not because a threshold was crossed.
        assert_eq!(sink, b"twelve bytes");
        assert_eq!(seen, vec![12]);
    }
}
