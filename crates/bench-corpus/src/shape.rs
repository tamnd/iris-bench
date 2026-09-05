//! Checking that a corpus is the size it is supposed to be, before anything measures it.
//!
//! `ClickBench` hits is 99,997,497 rows across 105 columns. A download that stops early and still
//! parses is the failure that quietly invalidates a month of results, and the digest catches that
//! for a file while saying nothing about a corpus assembled from the wrong set of files. The row
//! count is the check that covers the corpus rather than the file.
//!
//! The column count earns its place for a second reason. 105 is why a fixed size 64 bit projection
//! mask is not an acceptable ABI design, which is a finding this repository hands back to iris, and
//! a finding is worth more when the number behind it is asserted on every run than when it is
//! recalled from a page somebody read once.
//!
//! # What this reads
//!
//! The Parquet footer and nothing else. Row and column counts are metadata, so this is a seek and a
//! few kilobytes rather than a pass over the file, which is what makes it affordable to run before
//! every measurement rather than once when the corpus was added.

use std::{fs::File, path::Path};

use parquet::file::reader::{FileReader, SerializedFileReader};

use crate::{
    manifest::Manifest,
    store::{Store, StoreError},
};

/// How large a corpus turned out to be.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shape {
    /// How many rows, summed across every file.
    pub rows: u64,
    /// How many columns, which every file has to agree on.
    pub columns: u64,
}

/// Why a shape could not be read, or was read and was wrong.
#[derive(Debug, thiserror::Error)]
pub enum ShapeError {
    /// The file is not there or cannot be opened.
    #[error("opening {path}: {source}")]
    Open {
        /// Which file.
        path: String,
        /// What the filesystem said.
        source: std::io::Error,
    },
    /// The file is not the Parquet it is named as.
    #[error("reading the footer of {path}: {source}")]
    Parquet {
        /// Which file.
        path: String,
        /// What the Parquet reader objected to.
        source: Box<parquet::errors::ParquetError>,
    },
    /// Two files of one corpus disagree about how many columns it has.
    #[error(
        "{corpus} is not one table: {first} has {expected} columns and {second} has {found}. A \
         corpus whose files have different schemas cannot be scanned as one thing"
    )]
    Ragged {
        /// Which corpus.
        corpus: String,
        /// The file that set the expectation.
        first: String,
        /// How many columns it has.
        expected: u64,
        /// The file that broke it.
        second: String,
        /// How many columns that one has.
        found: u64,
    },
    /// The corpus loaded and is not the shape the manifest asserted.
    #[error(
        "{corpus} was asserted to have {wanted} {what} and has {found}. Either the download is not \
         what it was last time or the manifest is wrong, and both are worth stopping for"
    )]
    Mismatch {
        /// Which corpus.
        corpus: String,
        /// Rows or columns.
        what: &'static str,
        /// What the manifest asserted.
        wanted: u64,
        /// What the files actually have.
        found: u64,
    },
    /// The manifest asserts a shape and nothing in the corpus has one.
    #[error(
        "{corpus} asserts a {what} and none of its files are Parquet, so there is no footer to \
         check it against. Either the assertion belongs to a different corpus or the files do"
    )]
    Unreadable {
        /// Which corpus.
        corpus: String,
        /// Rows or columns.
        what: &'static str,
    },
    /// The store does not have a file the manifest names.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Whether this is a file a shape can be read out of.
///
/// Decided by the path the manifest declares rather than by looking at the bytes, because the
/// manifest is this repository's own statement about what a file is. A corpus of compressed text
/// like Silesia has no rows or columns and is not being guessed at here, while a `.parquet` file
/// that turns out not to be Parquet is a hard error rather than something quietly skipped.
fn is_parquet(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("parquet"))
}

/// Reads the row and column counts out of one Parquet file's footer.
///
/// # Errors
///
/// If the file cannot be opened or its footer cannot be read.
pub fn of_file(path: &Path) -> Result<Shape, ShapeError> {
    let shown = path.display().to_string();
    let file = File::open(path).map_err(|source| ShapeError::Open {
        path: shown.clone(),
        source,
    })?;
    let reader = SerializedFileReader::new(file).map_err(|source| ShapeError::Parquet {
        path: shown.clone(),
        source: Box::new(source),
    })?;
    let metadata = reader.metadata().file_metadata();
    Ok(Shape {
        // A negative row count would mean a Parquet writer wrote one, and taking it as zero here
        // would turn that into a corpus that silently asserts nothing.
        rows: u64::try_from(metadata.num_rows()).unwrap_or(0),
        columns: metadata.schema_descr().num_columns() as u64,
    })
}

/// Measures a whole corpus out of the store.
///
/// Rows are summed and columns have to agree, because a corpus split across files is one table and
/// files with different schemas are not one table however they are named. Files that are not
/// Parquet do not contribute, so a corpus of compressed text measures as nothing rather than as an
/// error, and a manifest that asserts a shape for such a corpus is caught by [`check`].
///
/// # Errors
///
/// If the store is missing a file, a footer cannot be read, or the files disagree on column count.
pub fn of_corpus(manifest: &Manifest, store: &Store) -> Result<Shape, ShapeError> {
    let mut rows = 0_u64;
    let mut columns: Option<(&str, u64)> = None;
    for entry in manifest
        .files
        .iter()
        .filter(|entry| is_parquet(&entry.path))
    {
        let path = store.path(&entry.blake3);
        if !path.is_file() {
            return Err(ShapeError::Store(StoreError::Missing(entry.blake3)));
        }
        let shape = of_file(&path)?;
        rows += shape.rows;
        match columns {
            None => columns = Some((&entry.path, shape.columns)),
            Some((first, expected)) if expected != shape.columns => {
                return Err(ShapeError::Ragged {
                    corpus: manifest.corpus.name.clone(),
                    first: first.to_owned(),
                    expected,
                    second: entry.path.clone(),
                    found: shape.columns,
                });
            }
            Some(_) => {}
        }
    }
    Ok(Shape {
        rows,
        columns: columns.map_or(0, |(_, count)| count),
    })
}

/// Reads the corpus and refuses it if it is not what the manifest asserted.
///
/// Returns the shape it read, so that a caller can print what it checked rather than only that it
/// checked something. A manifest with no assertions is not an error, it is a corpus nobody has
/// written the numbers down for yet, and the shape comes back all the same.
///
/// # Errors
///
/// If the corpus cannot be measured, or its row or column count is not what was asserted.
pub fn check(manifest: &Manifest, store: &Store) -> Result<Shape, ShapeError> {
    let readable = manifest.files.iter().any(|entry| is_parquet(&entry.path));
    let found = of_corpus(manifest, store)?;
    for (what, wanted, actual) in [
        ("rows", manifest.assertions.rows, found.rows),
        ("columns", manifest.assertions.columns, found.columns),
    ] {
        // An assertion nothing can be checked against is worse than no assertion, because it reads
        // on the page like something is being verified.
        if wanted.is_some() && !readable {
            return Err(ShapeError::Unreadable {
                corpus: manifest.corpus.name.clone(),
                what,
            });
        }
        if let Some(wanted) = wanted
            && wanted != actual
        {
            return Err(ShapeError::Mismatch {
                corpus: manifest.corpus.name.clone(),
                what,
                wanted,
                found: actual,
            });
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digest::Digest;

    const EMPTY_DIGEST: &str = "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262";

    fn manifest(files: &str, assertions: &str) -> Manifest {
        Manifest::parse(&format!(
            "[corpus]\n\
             name = \"example\"\n\
             description = \"A corpus that exists to have its shape checked\"\n\
             source = \"https://example.invalid/example.parquet\"\n\
             licence = \"Apache-2.0\"\n\
             category = \"fetch\"\n\
             {files}{assertions}"
        ))
        .unwrap()
    }

    /// A Parquet file with `columns` columns and `rows` rows, written to `path`.
    fn write_parquet(path: &Path, columns: usize, rows: usize) {
        use parquet::{
            basic::Type,
            file::{properties::WriterProperties, writer::SerializedFileWriter},
            schema::types::Type as SchemaType,
        };
        use std::sync::Arc;

        let fields: Vec<Arc<SchemaType>> = (0..columns)
            .map(|i| {
                Arc::new(
                    SchemaType::primitive_type_builder(&format!("c{i}"), Type::INT64)
                        .with_repetition(parquet::basic::Repetition::REQUIRED)
                        .build()
                        .unwrap(),
                )
            })
            .collect();
        let schema = Arc::new(
            SchemaType::group_type_builder("row")
                .with_fields(fields)
                .build()
                .unwrap(),
        );

        let file = File::create(path).unwrap();
        let mut writer =
            SerializedFileWriter::new(file, schema, Arc::new(WriterProperties::new())).unwrap();
        let mut group = writer.next_row_group().unwrap();
        let values: Vec<i64> = (0..i64::try_from(rows).unwrap()).collect();
        while let Some(mut column) = group.next_column().unwrap() {
            column
                .typed::<parquet::data_type::Int64Type>()
                .write_batch(&values, None, None)
                .unwrap();
            column.close().unwrap();
        }
        group.close().unwrap();
        writer.close().unwrap();
    }

    /// A store holding one Parquet file, and the digest it went in under.
    fn stored(columns: usize, rows: usize) -> (tempfile::TempDir, Store, Digest) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("store")).unwrap();
        let path = dir.path().join("part.parquet");
        write_parquet(&path, columns, rows);
        let digest = store.insert_file(&path).unwrap().digest;
        (dir, store, digest)
    }

    fn entry(path: &str, digest: &Digest) -> String {
        format!(
            "[[files]]\npath = \"{path}\"\nblake3 = \"{}\"\nbytes = 1024\n",
            digest.to_hex()
        )
    }

    #[test]
    fn the_shape_of_one_file_is_read_from_its_footer() {
        let (_dir, store, digest) = stored(7, 40);
        let manifest = manifest(&entry("part.parquet", &digest), "");
        assert_eq!(
            of_corpus(&manifest, &store).unwrap(),
            Shape {
                rows: 40,
                columns: 7
            }
        );
    }

    #[test]
    fn rows_sum_across_the_files_of_one_corpus() {
        let (dir, store, first) = stored(7, 40);
        let second_path = dir.path().join("part-2.parquet");
        write_parquet(&second_path, 7, 60);
        let second = store.insert_file(&second_path).unwrap().digest;

        let manifest = manifest(
            &format!(
                "{}{}",
                entry("part-1.parquet", &first),
                entry("part-2.parquet", &second)
            ),
            "",
        );
        assert_eq!(
            of_corpus(&manifest, &store).unwrap(),
            Shape {
                rows: 100,
                columns: 7
            }
        );
    }

    #[test]
    fn files_that_disagree_on_columns_are_not_one_corpus() {
        let (dir, store, first) = stored(7, 40);
        let second_path = dir.path().join("part-2.parquet");
        write_parquet(&second_path, 9, 40);
        let second = store.insert_file(&second_path).unwrap().digest;

        let manifest = manifest(
            &format!(
                "{}{}",
                entry("part-1.parquet", &first),
                entry("part-2.parquet", &second)
            ),
            "",
        );
        let error = of_corpus(&manifest, &store).unwrap_err();
        assert!(matches!(error, ShapeError::Ragged { .. }));
    }

    #[test]
    fn an_assertion_that_holds_passes() {
        let (_dir, store, digest) = stored(105, 40);
        let manifest = manifest(
            &entry("part.parquet", &digest),
            "[assertions]\nrows = 40\ncolumns = 105\n",
        );
        assert_eq!(check(&manifest, &store).unwrap().columns, 105);
    }

    #[test]
    fn a_row_count_that_does_not_hold_is_refused() {
        let (_dir, store, digest) = stored(7, 40);
        let manifest = manifest(
            &entry("part.parquet", &digest),
            "[assertions]\nrows = 99997497\n",
        );
        let message = format!("{}", check(&manifest, &store).unwrap_err());
        // Both numbers, because the interesting part of a truncated corpus is how short it is.
        assert!(message.contains("99997497"), "{message}");
        assert!(message.contains("40"), "{message}");
    }

    #[test]
    fn a_column_count_that_does_not_hold_is_refused() {
        let (_dir, store, digest) = stored(64, 40);
        let manifest = manifest(
            &entry("part.parquet", &digest),
            "[assertions]\ncolumns = 105\n",
        );
        assert!(matches!(
            check(&manifest, &store),
            Err(ShapeError::Mismatch {
                what: "columns",
                ..
            })
        ));
    }

    #[test]
    fn a_corpus_the_store_does_not_have_says_so_rather_than_reading_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let manifest = manifest(
            &entry("part.parquet", &EMPTY_DIGEST.parse().unwrap()),
            "[assertions]\nrows = 40\n",
        );
        assert!(matches!(
            check(&manifest, &store),
            Err(ShapeError::Store(StoreError::Missing(_)))
        ));
    }

    #[test]
    fn a_corpus_that_is_not_tabular_measures_as_nothing_rather_than_as_an_error() {
        // Silesia and enwik8 are compressed text. Reading a Parquet footer out of them is not a
        // check that fails, it is a check that does not apply.
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("store")).unwrap();
        let path = dir.path().join("silesia.tar");
        std::fs::write(&path, b"not parquet and not pretending to be").unwrap();
        let digest = store.insert_file(&path).unwrap().digest;

        let manifest = manifest(&entry("silesia.tar", &digest), "");
        assert_eq!(
            check(&manifest, &store).unwrap(),
            Shape {
                rows: 0,
                columns: 0
            }
        );
    }

    #[test]
    fn asserting_a_shape_for_a_corpus_that_has_none_is_refused() {
        // Worse than asserting nothing, because on the page it reads like something is being
        // verified, and what would actually happen is a comparison against zero.
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("store")).unwrap();
        let path = dir.path().join("silesia.tar");
        std::fs::write(&path, b"not parquet and not pretending to be").unwrap();
        let digest = store.insert_file(&path).unwrap().digest;

        let manifest = manifest(&entry("silesia.tar", &digest), "[assertions]\nrows = 40\n");
        assert!(matches!(
            check(&manifest, &store),
            Err(ShapeError::Unreadable { .. })
        ));
    }

    #[test]
    fn a_file_named_parquet_that_is_not_parquet_is_a_hard_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("store")).unwrap();
        let path = dir.path().join("part.parquet");
        std::fs::write(&path, b"claims to be parquet and is not").unwrap();
        let digest = store.insert_file(&path).unwrap().digest;

        let manifest = manifest(&entry("part.parquet", &digest), "");
        assert!(matches!(
            check(&manifest, &store),
            Err(ShapeError::Parquet { .. })
        ));
    }

    #[test]
    fn a_manifest_with_no_assertions_still_reports_what_it_read() {
        let (_dir, store, digest) = stored(7, 40);
        let manifest = manifest(&entry("part.parquet", &digest), "");
        assert_eq!(check(&manifest, &store).unwrap().rows, 40);
    }
}
