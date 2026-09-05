//! The M4 gate: a scan of a resident local file against a scan of the same bytes already in memory.
//!
//! iris reads a file through a window of fixed address space that slides as a scan moves through
//! the file. The claim that makes that design worth having is that when the file is already in the
//! page cache, the window costs almost nothing next to handing the whole file to the scan as a
//! buffer. This measures how much almost nothing is.
//!
//! It matters because the windowed path is the one that also works when the file does not fit in
//! memory and when the bytes are in a bucket rather than on a disk. If it is free on the easy case
//! then there is one code path for every case. If it is not free, then hosts will keep a second
//! path for resident files, and every bug found in one of them has to be looked for in the other.
//!
//! # Why this is measured here and not in iris
//!
//! `iris-source` is a dependency of this crate, taken from crates.io like any other consumer would
//! take it. That is deliberate. A benchmark living inside the repository it measures can reach
//! internals, and reaching one internal is all it takes for the measured path to stop being the
//! path a user gets. Everything here goes through [`RangeSource`], which is the trait a host has.
//!
//! # The comparison
//!
//! Both sides scan in ranges of the same size through the same trait, so the only difference
//! between them is which implementation answers. The alternative, comparing a windowed scan against
//! a flat loop over a slice, would fold the cost of the trait into the answer and report it as the
//! cost of the window.
//!
//! Samples are taken in pairs, one of each, back to back, and the pair order alternates. Taken back
//! to back so that a machine drifting during the run drifts under both sides and divides out, which
//! is what [`paired_ratio`] is built to exploit. Alternating so that whichever side goes first does
//! not collect a systematic advantage or a systematic penalty from being the one that runs on a
//! freshly descheduled core.
//!
//! # What this comparison gives away
//!
//! The buffer is filled once, before anything is timed, and then scanned over and over. So the
//! whole buffer path pays for its allocation and its first touch of every page exactly once and
//! never again, while the windowed path re-establishes a mapping every time the window moves. That
//! is a real asymmetry and it favours the buffer, and it is stated here rather than in a footnote
//! because it is large enough to be the answer.
//!
//! It is kept rather than corrected because the alternative is worse. Timing the setup of each side
//! would put a 256 MiB copy from the page cache into the buffer path, which no host doing a single
//! scan would pay and which would flatter the window instead. Steady state is the case where the
//! two designs are genuinely comparable, so that is the one measured, and the reader is told which
//! way the remaining bias runs.
//!
//! # What the work is
//!
//! Summing the bytes as little endian 64 bit words. It is deliberately the cheapest per byte thing
//! a scan can plausibly do, because the cost being measured is the source underneath and any real
//! work piled on top only buries it. A ratio measured with expensive work in the loop would be
//! closer to parity and would mean less: it would say the window is cheap next to the work, which
//! nobody doubted, rather than that the window is cheap.

use std::path::Path;

use anyhow::{Context as _, bail};
use bench_core::{Bootstrap, Ratio, Summary, paired_ratio, summarise, time};
use bench_env::{Capture, Outcome, Permit};
use iris_source::{Fetch, FileSource, MemorySource, RangeSource};

/// An odd multiplier with its bits spread out, so the generated file has no short cycle in it.
const MULTIPLIER: u64 = 6_364_136_223_846_793_005;

/// What one pair of measurements came to, in nanoseconds.
#[derive(Clone, Copy, Debug)]
struct Pair {
    windowed: f64,
    buffered: f64,
}

/// Writes a file of `bytes` bytes with content that does not compress and does not repeat.
///
/// Content matters less here than it would for a decoder, since the scan reads every byte either
/// way, but a file of zeroes is the one shape where a filesystem is allowed to not store it.
fn write_file(path: &Path, bytes: u64) -> anyhow::Result<()> {
    let mut word = MULTIPLIER;
    let mut out = Vec::with_capacity(usize::try_from(bytes).unwrap_or(usize::MAX));
    while (out.len() as u64) < bytes {
        word = word.wrapping_mul(MULTIPLIER).wrapping_add(1);
        out.extend_from_slice(&word.to_le_bytes());
    }
    out.truncate(usize::try_from(bytes).unwrap_or(usize::MAX));
    std::fs::write(path, &out).context("writing the file to scan")?;
    Ok(())
}

/// Sums a range of bytes as little endian 64 bit words, with the tail folded in a byte at a time.
fn fold(bytes: &[u8]) -> u64 {
    let (words, tail) = bytes.as_chunks::<8>();
    let mut sum = words.iter().fold(0u64, |sum, word| {
        sum.wrapping_add(u64::from_le_bytes(*word))
    });
    for &byte in tail {
        sum = sum.wrapping_add(u64::from(byte));
    }
    sum
}

/// Scans a whole source in `chunk` sized ranges and returns what it summed.
///
/// The sum is returned rather than dropped so that nothing here is dead code a compiler is entitled
/// to delete, which would turn this into a measurement of an empty loop.
fn scan<S: RangeSource>(source: &mut S, chunk: usize) -> anyhow::Result<u64> {
    let len = source.len();
    let mut at = 0u64;
    let mut sum = 0u64;
    while at < len {
        let want = usize::try_from(len - at).unwrap_or(usize::MAX).min(chunk);
        match source.range(at, want).context("reading a range")? {
            Fetch::Ready(bytes) => sum = sum.wrapping_add(fold(bytes)),
            // Neither source here is ever pending. A mapped file blocks the thread on a page fault
            // and a resident buffer never blocks at all, so reaching this means the source under
            // test is not the one this command was written for. The arm is a catch all rather than
            // naming `Pending`, because [`Fetch`] is non exhaustive and a variant added upstream
            // should turn into this refusal rather than into a build failure here.
            _ => bail!("a source that does not answer with bytes cannot be timed this way"),
        }
        at += want as u64;
    }
    Ok(sum)
}

/// Runs the gate.
///
/// # Errors
///
/// If the machine is not fit to produce a ratio and `anyway` was not asked for, if the file cannot
/// be written or opened, or if the measured interval does not clear `bar`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn gate(
    size: u64,
    span: usize,
    chunk: usize,
    pairs: u32,
    warmup: u32,
    bar: f64,
    anyway: bool,
) -> anyhow::Result<()> {
    if chunk > span {
        bail!(
            "a chunk of {chunk} bytes cannot be served by a window with a span of {span}, so this \
             would measure the error path rather than the scan"
        );
    }

    let capture = Capture::take();
    unfit(&capture, anyway, "before the measurement started")?;

    let directory = tempfile::tempdir().context("making somewhere to put the file")?;
    let path = directory.path().join("scan.bin");
    write_file(&path, size)?;

    // One read serves two purposes: it is the buffer the whole buffer path scans, and it is what
    // pulls the file into the page cache so that the windowed path is measured resident, which is
    // the case this gate is about. A cold file is a measurement of the disk.
    let resident = std::fs::read(&path).context("reading the file back to make it resident")?;

    let mut file = FileSource::with_span(
        std::fs::File::open(&path).context("opening the file to scan")?,
        span,
    )
    .context("reserving the window")?;
    let mut memory = MemorySource::new(resident);

    for _ in 0..warmup {
        scan(&mut file, chunk)?;
        scan(&mut memory, chunk)?;
    }

    // Slides are counted from the point the warmup ends, because the count on the source is for the
    // life of the window and the number a reader wants is what one scan costs.
    let before_slides = file.slides();

    let mut measured = Vec::with_capacity(pairs as usize);
    for pair in 0..pairs {
        // Whichever side goes first pays for whatever the other one left in the caches. Alternating
        // means that cost lands on both sides equally instead of on one of them every time.
        let (windowed, buffered) = if pair.is_multiple_of(2) {
            let windowed = time(|| scan(&mut file, chunk)).1;
            let buffered = time(|| scan(&mut memory, chunk)).1;
            (windowed, buffered)
        } else {
            let buffered = time(|| scan(&mut memory, chunk)).1;
            let windowed = time(|| scan(&mut file, chunk)).1;
            (windowed, buffered)
        };
        measured.push(Pair { windowed, buffered });
    }

    let after = Capture::take();
    unfit(&after, anyway, "by the time the measurement finished")?;

    let windowed: Vec<f64> = measured.iter().map(|pair| pair.windowed).collect();
    let buffered: Vec<f64> = measured.iter().map(|pair| pair.buffered).collect();

    let Some(ratio) = paired_ratio(&windowed, &buffered, Bootstrap::default()) else {
        bail!("{pairs} pairs is not enough to compare, try at least one");
    };
    let window_summary = summarise(&windowed, Bootstrap::default()).expect("at least one pair");
    let buffer_summary = summarise(&buffered, Bootstrap::default()).expect("at least one pair");

    report(
        &capture,
        size,
        span,
        chunk,
        pairs,
        &window_summary,
        &buffer_summary,
        &ratio,
        bar,
        (file.slides() - before_slides) / u64::from(pairs),
    );

    if anyway {
        println!();
        println!(
            "this was measured with a gate failing, so it describes today on this machine and not \
             the machine"
        );
    }

    if ratio.within(bar) {
        Ok(())
    } else {
        // Unlike the noise floor, being over the bar here is a failure and exits non zero. The
        // floor measures a property of a machine that nobody chose and being surprised by it is the
        // reason to run it. This measures a claim the project makes about its own design, and a
        // claim that stops holding should stop a build rather than print a number nobody reads.
        bail!(
            "the windowed path is {:.2}% of the whole buffer path, interval {:.2}% to {:.2}%, and \
             the bar is within {:.2}%. Either the window got more expensive or the measurement is \
             too weak to tell, and `n` and the interval width say which. If the scan slides, run it \
             again with a span that holds the whole file: a gap that closes when the sliding stops \
             is the cost of re-establishing the mapping and not the cost of the abstraction",
            ratio.ratio * 100.0,
            ratio.lo * 100.0,
            ratio.hi * 100.0,
            bar * 100.0
        )
    }
}

/// Prints what was measured.
#[allow(clippy::too_many_arguments)]
fn report(
    capture: &Capture,
    size: u64,
    span: usize,
    chunk: usize,
    pairs: u32,
    windowed: &Summary,
    buffered: &Summary,
    ratio: &Ratio,
    bar: f64,
    slides: u64,
) {
    let mib = 1024 * 1024;
    println!("{}", capture.class);
    println!("environment {}", capture.hash);
    println!(
        "{pairs} pairs over a {} MiB file, a {} MiB window and {} KiB ranges, {slides} slides per \
         scan",
        size / mib,
        span as u64 / mib,
        chunk / 1024
    );
    println!();
    println!(
        "  windowed file       {:>8.3} ms   {:.3} to {:.3}",
        windowed.median / 1e6,
        windowed.lo / 1e6,
        windowed.hi / 1e6
    );
    println!(
        "  whole buffer        {:>8.3} ms   {:.3} to {:.3}",
        buffered.median / 1e6,
        buffered.lo / 1e6,
        buffered.hi / 1e6
    );
    println!(
        "  ratio               {:>8.2}%   {:.2}% to {:.2}% at {:.0}% confidence",
        ratio.ratio * 100.0,
        ratio.lo * 100.0,
        ratio.hi * 100.0,
        ratio.confidence * 100.0
    );
    println!();
    if ratio.within(bar) {
        println!(
            "the whole interval is within {:.2}% of the whole buffer path, so the gate holds",
            bar * 100.0
        );
    } else {
        println!(
            "the interval runs outside {:.2}% of the whole buffer path, so the gate does not hold",
            bar * 100.0
        );
    }
    println!(
        "this machine is good for {}, and this number is a ratio taken inside one run",
        capture.permits()
    );
    if capture.permits() == Permit::Nothing {
        println!(
            "so it is a reading and not a result, and it belongs in a working note rather than a \
             table"
        );
    }
}

/// Stops the measurement when the machine is not fit to produce a ratio.
fn unfit(capture: &Capture, anyway: bool, when: &str) -> anyhow::Result<()> {
    let Some(gate) = capture.failed() else {
        return Ok(());
    };
    if anyway {
        return Ok(());
    }
    let detail = match &gate.outcome {
        Outcome::Pass { observed } => observed.clone(),
        Outcome::Fail { observed, wanted } => format!("{observed}, wanted {wanted}"),
    };
    bail!(
        "the {} gate failed {when}: {detail}. A three percent bar sits under the noise floor of \
         every machine in the fleet except a quiet pinned one, so a reading taken here would be \
         the something else that was running. Wait for the machine to settle, or ask for --anyway \
         if a working note is what is wanted",
        gate.name
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_sources_read_the_same_bytes() {
        // The whole comparison is worthless if the two sides are not scanning the same thing, and
        // an off by one in the chunk arithmetic would show up here and nowhere else.
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("scan.bin");
        write_file(&path, 3 * 1024 * 1024 + 17).unwrap();

        let resident = std::fs::read(&path).unwrap();
        let mut memory = MemorySource::new(resident);
        let mut file =
            FileSource::with_span(std::fs::File::open(&path).unwrap(), 1024 * 1024).unwrap();

        let through_buffer = scan(&mut memory, 64 * 1024).unwrap();
        let through_window = scan(&mut file, 64 * 1024).unwrap();
        assert_eq!(through_buffer, through_window);
    }

    #[test]
    fn a_file_that_does_not_divide_by_the_chunk_still_reads_to_the_end() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("scan.bin");
        let size = 1_000_003;
        write_file(&path, size).unwrap();

        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.len() as u64, size);

        let mut memory = MemorySource::new(bytes.clone());
        assert_eq!(scan(&mut memory, 4096).unwrap(), fold(&bytes));
    }

    #[test]
    fn a_window_smaller_than_a_chunk_is_refused_before_anything_is_measured() {
        let error = gate(1024 * 1024, 4096, 8192, 1, 0, 0.03, true).unwrap_err();
        assert!(format!("{error}").contains("cannot be served by a window"));
    }
}
