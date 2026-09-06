//! `iris-bench corpus`, which gets a corpus onto the machine and then refuses to believe it.
//!
//! Three things happen in order and the order is the point. The manifest is read and validated, so
//! a corpus that describes itself badly never reaches the network or a generator. The files are
//! fetched or generated and hashed in the same pass, so bytes that are not what was promised never
//! reach the store. The footers are read and checked against the asserted row and column counts, so
//! a corpus that is short never reaches a measurement.
//!
//! The last of those is the one worth being explicit about. A digest catches a file that changed
//! and says nothing about a corpus assembled from the wrong set of files, and a download that stops
//! early and still parses is exactly the failure that quietly invalidates a month of results.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};
use bench_corpus::{Category, Manifest, Progress, Shape, Store, fetch, generate, shape};

/// Where corpora are described, relative to the repository root.
const CORPORA: &str = "corpora";

/// Where the bytes go when nothing else says.
const DEFAULT_STORE: &str = "corpus";

/// Where a generator is allowed to write when nothing else says.
const DEFAULT_SCRATCH: &str = "corpus-scratch";

/// What a corpus did to one file, whichever way it arrived.
struct Arrived {
    /// The path inside the corpus.
    path: String,
    /// What the file is addressed by.
    digest: bench_corpus::Digest,
    /// Whether the store already had it, in which case nothing arrived at all.
    deduplicated: bool,
}

/// Reads a corpus manifest, gets what it names, and checks what arrived.
pub(crate) fn run(
    name: &str,
    root: Option<PathBuf>,
    store: Option<PathBuf>,
    generator: Option<&Path>,
    scratch: Option<PathBuf>,
) -> anyhow::Result<()> {
    let root = root.unwrap_or_else(|| PathBuf::from(CORPORA));
    let path = root.join(name).join("manifest.toml");
    if !path.is_file() {
        bail!(
            "no manifest at {}. The corpora in this tree are {}",
            path.display(),
            available(&root)
        );
    }

    let manifest = Manifest::read(&path)?;
    println!("{}, {}", manifest.corpus.name, manifest.corpus.description);
    println!(
        "  {} under {}, {} in {} file{}",
        manifest.corpus.category,
        manifest.corpus.licence,
        bytes(manifest.bytes()),
        manifest.files.len(),
        if manifest.files.len() == 1 { "" } else { "s" }
    );
    println!("  from {}", manifest.corpus.source);

    let store = Store::open(store.unwrap_or_else(|| PathBuf::from(DEFAULT_STORE)))
        .context("opening the corpus store")?;
    println!("  into {}", store.root().display());
    println!();

    let mut last = String::new();
    let arrived = match manifest.corpus.category {
        Category::Generate => {
            let scratch = scratch.unwrap_or_else(|| PathBuf::from(DEFAULT_SCRATCH));
            generate::corpus(&manifest, &store, generator, &scratch, &mut |progress| {
                report(&mut last, &progress);
            })?
            .into_iter()
            .map(|file| Arrived {
                path: file.path,
                digest: file.digest,
                deduplicated: file.deduplicated,
            })
            .collect()
        }
        _ => fetch::corpus(&manifest, &store, &mut |progress| {
            report(&mut last, &progress);
        })?
        .into_iter()
        .map(|file| Arrived {
            path: file.path,
            digest: file.digest,
            deduplicated: file.deduplicated,
        })
        .collect::<Vec<_>>(),
    };

    for file in &arrived {
        println!(
            "  {}  {}  {}{}",
            if file.deduplicated { "have" } else { "got " },
            file.digest,
            file.path,
            if file.deduplicated {
                ", already in the store"
            } else {
                ""
            }
        );
    }
    println!();

    describe(&manifest, shape::check(&manifest, &store)?);
    Ok(())
}

/// Says what the shape check looked at and, more importantly, what it did not.
fn describe(manifest: &Manifest, found: Shape) {
    let tabular = manifest
        .files
        .iter()
        .any(|entry| entry.path.to_ascii_lowercase().ends_with(".parquet"));
    if !tabular {
        println!("  nothing here is tabular, so there is no row or column count to check");
        return;
    }

    println!("  {} rows and {} columns", found.rows, found.columns);
    match (manifest.assertions.rows, manifest.assertions.columns) {
        (Some(_), Some(_)) => println!("  both are what the manifest asserts"),
        // Said out loud rather than left to be inferred from silence. A number that was read is
        // printed exactly like a number that was checked, and only one of them would have caught a
        // download that stopped early.
        (None, None) => println!(
            "  note: this manifest asserts neither, so both were read rather than checked and a \
             short download would have passed"
        ),
        (rows, _) => println!(
            "  note: this manifest asserts the {} count and not the other, so only one of them was \
             checked",
            if rows.is_some() { "row" } else { "column" }
        ),
    }
}

/// Prints how far along a download is, on one line that keeps being rewritten.
fn report(last: &mut String, progress: &Progress<'_>) {
    let line = format!(
        "  {} of {}  {}",
        bytes(progress.done),
        bytes(progress.total),
        progress.path
    );
    if line == *last {
        return;
    }
    // Carriage return rather than a newline, because a fifteen gigabyte download reporting every
    // sixty four mebibytes is two hundred lines of scrollback nobody wants.
    print!("\r{line}    ");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    if progress.done >= progress.total {
        println!();
    }
    *last = line;
}

/// What corpora are in the tree, for the message when somebody asks for one that is not.
fn available(root: &Path) -> String {
    let Ok(entries) = std::fs::read_dir(root) else {
        return format!("not readable at {}", root.display());
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join("manifest.toml").is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    if names.is_empty() {
        format!("none, {} has no manifests in it", root.display())
    } else {
        names.join(", ")
    }
}

/// A byte count a person can read at a glance.
fn bytes(count: u64) -> String {
    #[allow(
        clippy::cast_precision_loss,
        reason = "this is a label on a progress line, not a measurement"
    )]
    let mut value = count as f64;
    for unit in ["B", "KiB", "MiB", "GiB"] {
        if value < 1024.0 {
            return if unit == "B" {
                format!("{count} B")
            } else {
                format!("{value:.1} {unit}")
            };
        }
        value /= 1024.0;
    }
    format!("{value:.1} TiB")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_counts_read_the_way_a_person_would_say_them() {
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(2048), "2.0 KiB");
        assert_eq!(bytes(14_779_976_446), "13.8 GiB");
    }

    #[test]
    fn asking_for_a_corpus_that_is_not_there_lists_the_ones_that_are() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("clickbench-hits")).unwrap();
        std::fs::write(dir.path().join("clickbench-hits").join("manifest.toml"), "").unwrap();

        let error = run(
            "not-a-corpus",
            Some(dir.path().to_owned()),
            None,
            None,
            None,
        )
        .unwrap_err();
        assert!(format!("{error}").contains("clickbench-hits"));
    }

    #[test]
    fn a_directory_with_no_manifest_in_it_is_not_offered() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("half-finished")).unwrap();
        assert!(available(dir.path()).contains("no manifests"));
    }
}
