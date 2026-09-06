//! What a corpus is, written down and checkable.
//!
//! A corpus that is not pinned by digest is not a corpus, it is whatever happened to download that
//! day. A manifest says where the bytes come from, what licence they arrive under, which of the
//! three handling categories in `docs/LICENSING.md` applies, and the digest of every file. Nothing
//! is measured on data that does not have one.
//!
//! The format is documented in `docs/CORPORA.md`. The rules below are the same rules
//! `ci/discipline.py` applies to every manifest in the tree, deliberately duplicated rather than
//! shared, because the Python one runs on a repository that may not compile and the Rust one runs
//! on a manifest that may not be in this repository at all.

use std::{fmt, path::Path};

use serde::{Deserialize, Serialize};

use crate::digest::Digest;

/// A whole manifest, as it is written in `corpora/<name>/manifest.toml`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// What this corpus is and where it comes from.
    pub corpus: Corpus,
    /// One entry per file, each pinned by digest.
    #[serde(default)]
    pub files: Vec<Entry>,
    /// What produces the bytes, which a generated corpus has and no other kind may.
    #[serde(default)]
    pub generator: Option<Generator>,
    /// Facts about the loaded corpus that a run checks before it measures anything.
    #[serde(default)]
    pub assertions: Assertions,
}

/// The corpus itself, as opposed to the files it is made of.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Corpus {
    /// The identifier, which is also the directory name.
    pub name: String,
    /// One sentence on what this is, for a reader who has not met it before.
    pub description: String,
    /// Where the bytes come from, as a URL or as the generator that produces them.
    pub source: String,
    /// The licence the data arrives under, as an SPDX identifier where one exists.
    pub licence: String,
    /// How this repository handles the data.
    pub category: Category,
    /// What permits redistribution, which only a mirrored corpus needs and must have.
    #[serde(default)]
    pub licence_note: Option<String>,
    /// The corpus this one is a selection from, for a corpus published in more than one part.
    #[serde(default)]
    pub part_of: Option<String>,
}

/// How this repository handles a corpus, per `docs/LICENSING.md`.
///
/// The default is [`Category::Fetch`]. Mirroring requires reading the licence and writing down what
/// it permits, which is what `licence_note` is and why it is checked rather than suggested.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    /// Produced locally from a deterministic generator. Nothing is redistributed.
    Generate,
    /// Downloaded from its canonical location at run time and never redistributed by us.
    Fetch,
    /// Redistributed here, because the licence permits it and a broken link in three years is the
    /// difference between a reproducible result and a story about one.
    Mirror,
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Generate => "generate",
            Self::Fetch => "fetch",
            Self::Mirror => "mirror",
        })
    }
}

/// One file of a corpus.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// Where the file sits inside the corpus, relative to its own directory.
    pub path: String,
    /// The digest of the file's bytes.
    pub blake3: Digest,
    /// How large the file is, which is checked before the digest because it is free.
    pub bytes: u64,
    /// Where this particular file comes from, when the corpus source does not already say.
    ///
    /// Left out for almost every corpus. It exists for the case where the files of one corpus are
    /// not all under one prefix, which is common enough in published datasets that the format
    /// having no answer for it would mean forking the corpus rather than describing it.
    #[serde(default)]
    pub url: Option<String>,
}

/// What produces a generated corpus, written down closely enough to run.
///
/// A generated corpus is pinned by digest like every other kind, and the digest of what a generator
/// writes depends on which generator it was. So the version is part of the pin and is checked before
/// anything runs, because the alternative is a digest mismatch on ten gigabytes of output with no
/// indication that the cause is a build of `dbgen` two releases along.
///
/// The program itself is never in this repository. TPC's tools are obtained by the person running
/// the generation, under TPC's own terms, and `docs/LICENSING.md` says why.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Generator {
    /// What to run, found on `PATH` unless the caller supplies a path.
    pub program: String,
    /// What to pass it.
    #[serde(default)]
    pub arguments: Vec<String>,
    /// The version this corpus was generated with, which must appear in the program's own banner.
    pub version: String,
    /// What to pass the program to make it print that banner.
    #[serde(default = "Generator::default_version_arguments")]
    pub version_arguments: Vec<String>,
    /// Environment the program needs, where `{output}` is the directory the files should land in
    /// and `{program_directory}` is the directory the program itself was found in.
    #[serde(default)]
    pub environment: std::collections::BTreeMap<String, String>,
}

impl Generator {
    /// What to pass a program to make it say what it is, when the manifest does not say.
    fn default_version_arguments() -> Vec<String> {
        vec!["-h".to_owned()]
    }
}

/// Facts a run checks about the loaded corpus before it measures anything.
///
/// `ClickBench` is 99,997,497 rows across 105 columns or the manifest is wrong, and finding that out
/// from an assertion is much cheaper than finding it out from a number that looks slightly off.
/// Every field is optional because not every corpus is tabular.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assertions {
    /// How many rows the corpus has once loaded.
    #[serde(default)]
    pub rows: Option<u64>,
    /// How many columns the corpus has once loaded.
    #[serde(default)]
    pub columns: Option<u64>,
}

/// Why a manifest is not usable.
#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    /// The file could not be read.
    #[error("reading {path}: {source}")]
    Read {
        /// Which file.
        path: String,
        /// What went wrong underneath.
        source: std::io::Error,
    },
    /// The file is not the TOML this format is.
    #[error("parsing {path}: {source}")]
    Parse {
        /// Which file.
        path: String,
        /// What the TOML parser objected to.
        source: Box<toml::de::Error>,
    },
    /// The file parsed but says something that cannot be true.
    #[error("{name}: {problem}")]
    Invalid {
        /// Which corpus.
        name: String,
        /// What is wrong with it.
        problem: String,
    },
}

impl Manifest {
    /// Reads and validates a manifest from a path.
    ///
    /// The directory the manifest sits in is the corpus name, and a manifest that disagrees with
    /// its own directory is refused. Two names for one corpus is how a run ends up citing a
    /// corpus that is not the one it read.
    ///
    /// # Errors
    ///
    /// If the file cannot be read, is not valid TOML for this format, or fails [`Self::validate`].
    pub fn read(path: &Path) -> Result<Self, ManifestError> {
        let shown = path.display().to_string();
        let text = std::fs::read_to_string(path).map_err(|source| ManifestError::Read {
            path: shown.clone(),
            source,
        })?;
        let manifest = Self::parse(&text).map_err(|error| match error {
            ManifestError::Parse { source, .. } => ManifestError::Parse {
                path: shown.clone(),
                source,
            },
            other => other,
        })?;
        if let Some(directory) = path.parent().and_then(|parent| parent.file_name())
            && directory != manifest.corpus.name.as_str()
        {
            return Err(ManifestError::Invalid {
                name: manifest.corpus.name.clone(),
                problem: format!(
                    "the manifest names this corpus {} and it lives in a directory called {}",
                    manifest.corpus.name,
                    directory.to_string_lossy()
                ),
            });
        }
        Ok(manifest)
    }

    /// Parses and validates a manifest from TOML.
    ///
    /// # Errors
    ///
    /// If the text is not valid TOML for this format, or fails [`Self::validate`].
    pub fn parse(text: &str) -> Result<Self, ManifestError> {
        let manifest: Self = toml::from_str(text).map_err(|source| ManifestError::Parse {
            path: "<manifest>".to_owned(),
            source: Box::new(source),
        })?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// The one digest that names this exact corpus, contents and all.
    ///
    /// Public BI is why this exists. The benchmark is published as 206 tables and much of the
    /// encoding literature measures a 36 table subset, so a result labelled Public BI is ambiguous
    /// about which Public BI, and a result labelled with the subset is ambiguous about which 36.
    /// This is a single value that answers both, small enough to sit in a result row next to the
    /// name and specific enough that no two selections share one.
    ///
    /// Taken over the name and then every entry's path, digest and size, sorted by path, so it does
    /// not move when entries are reordered in the file and does move when any byte of the corpus
    /// does. Deliberately not the digest of the manifest text: a comment rewritten is not a
    /// different corpus, and a result row that changed because somebody fixed a typo would teach
    /// everybody to ignore the field.
    #[must_use]
    pub fn identity(&self) -> Digest {
        let mut entries: Vec<&Entry> = self.files.iter().collect();
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        let mut hasher = blake3::Hasher::new();
        hasher.update(self.corpus.name.as_bytes());
        hasher.update(b"\n");
        for entry in entries {
            hasher.update(entry.path.as_bytes());
            hasher.update(b" ");
            hasher.update(entry.blake3.to_hex().as_bytes());
            hasher.update(b" ");
            hasher.update(entry.bytes.to_string().as_bytes());
            hasher.update(b"\n");
        }
        Digest::from_bytes(*hasher.finalize().as_bytes())
    }

    /// Whether this corpus really is a selection from the one it says it is part of.
    ///
    /// A subset is only worth naming if it is provably a subset. Public BI is published as 206
    /// tables and much of the encoding literature measures 36 of them, and a 36 that has quietly
    /// drifted from the 206 it names is worse than no subset at all, because every number labelled
    /// with it is then about something nobody can reconstruct.
    ///
    /// Each entry has to be in the whole at the same path with the same digest and the same size,
    /// and the part has to be smaller. Names are not compared here, so this answers the question
    /// about the bytes and `part_of` answers the one about intent.
    #[must_use]
    pub fn is_part_of(&self, whole: &Self) -> bool {
        self.files.len() < whole.files.len()
            && self.files.iter().all(|entry| {
                whole.files.iter().any(|held| {
                    held.path == entry.path
                        && held.blake3 == entry.blake3
                        && held.bytes == entry.bytes
                })
            })
    }

    /// Checks everything about a manifest that parsing does not.
    ///
    /// Parsing gets the shape right. This gets the meaning right, and the difference is that a
    /// manifest can be perfectly well formed TOML and still pin nothing at all.
    ///
    /// # Errors
    ///
    /// If the corpus has no name, pins no files, names one file twice, uses a path that could
    /// escape the corpus directory, declares an empty file, or is mirrored without saying what
    /// permits it.
    pub fn validate(&self) -> Result<(), ManifestError> {
        let name = self.corpus.name.clone();
        let invalid = |problem: String| ManifestError::Invalid {
            name: name.clone(),
            problem,
        };

        if self.corpus.name.trim().is_empty() {
            return Err(invalid("the corpus has no name".to_owned()));
        }
        for (field, value) in [
            ("description", &self.corpus.description),
            ("source", &self.corpus.source),
            ("licence", &self.corpus.licence),
        ] {
            if value.trim().is_empty() {
                return Err(invalid(format!("{field} is empty")));
            }
        }

        // A mirrored corpus is the only one where this repository is the party redistributing, so
        // it is the only one where somebody has to have read the licence and written down what it
        // permits. Checked rather than asked for, because the point at which it gets skipped is
        // exactly the point at which somebody is in a hurry.
        if self.corpus.category == Category::Mirror
            && self
                .corpus
                .licence_note
                .as_ref()
                .is_none_or(|note| note.trim().is_empty())
        {
            return Err(invalid(
                "mirrored without a licence note saying what permits it".to_owned(),
            ));
        }

        // Whether the entries really are a subset needs the other manifest and is checked where
        // both are in hand, by `ci/discipline.py` over the tree and by `Self::is_part_of` at run
        // time. What can be settled from one manifest alone is that the claim is not circular.
        if let Some(whole) = self.corpus.part_of.as_ref() {
            if whole.trim().is_empty() {
                return Err(invalid(
                    "says it is part of a corpus with no name".to_owned(),
                ));
            }
            if whole == &self.corpus.name {
                return Err(invalid("says it is part of itself".to_owned()));
            }
        }

        // A generated corpus says what produces it and every other kind says where it is downloaded
        // from, and a manifest carrying both would be making two claims about one set of bytes. The
        // second of those is the one that would go unread.
        match (self.corpus.category, self.generator.as_ref()) {
            (Category::Generate, None) => {
                return Err(invalid(
                    "generated without a [generator], so nothing says how".to_owned(),
                ));
            }
            (category, Some(_)) if category != Category::Generate => {
                return Err(invalid(format!(
                    "has a [generator] and is a {category} corpus, which are two different \
                     answers to where the bytes come from"
                )));
            }
            _ => {}
        }
        if let Some(generator) = self.generator.as_ref() {
            generator.validate(&invalid)?;
        }

        if self.files.is_empty() {
            return Err(invalid(
                "no files, so this manifest pins nothing".to_owned(),
            ));
        }

        let mut seen: Vec<&str> = Vec::with_capacity(self.files.len());
        for entry in &self.files {
            entry.validate(&invalid)?;
            if seen.contains(&entry.path.as_str()) {
                return Err(invalid(format!("{} is listed twice", entry.path)));
            }
            seen.push(&entry.path);
        }

        Ok(())
    }

    /// How large the whole corpus is, which is what a run checks it has room for.
    #[must_use]
    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|entry| entry.bytes).sum()
    }
}

impl Generator {
    /// Checks a generator, reporting through the caller's error constructor.
    fn validate(&self, invalid: &impl Fn(String) -> ManifestError) -> Result<(), ManifestError> {
        for (field, value) in [("program", &self.program), ("version", &self.version)] {
            if value.trim().is_empty() {
                return Err(invalid(format!("the generator has no {field}")));
            }
        }
        // Without something to run the version probe on, the version in the manifest is a comment.
        // It is the field that turns a mismatch from a mystery into a sentence, so an empty probe
        // is refused rather than skipped.
        if self.version_arguments.is_empty() {
            return Err(invalid(
                "the generator has no version_arguments, so its version cannot be checked"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

impl Entry {
    /// Checks one file entry, reporting through the caller's error constructor.
    fn validate(&self, invalid: &impl Fn(String) -> ManifestError) -> Result<(), ManifestError> {
        if self.path.trim().is_empty() {
            return Err(invalid("a file entry has no path".to_owned()));
        }

        // A corpus path is a name inside the corpus directory and nothing else. An absolute path or
        // one containing a parent segment would let a manifest write outside the tree it describes,
        // and a manifest is a file fetched from somewhere else as often as it is one somebody here
        // wrote.
        let path = Path::new(&self.path);
        if path.is_absolute() {
            return Err(invalid(format!("{} is an absolute path", self.path)));
        }
        if path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            return Err(invalid(format!(
                "{} climbs out of the corpus directory",
                self.path
            )));
        }
        if self.bytes == 0 {
            return Err(invalid(format!(
                "{} is declared as zero bytes, which pins nothing",
                self.path
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY_DIGEST: &str = "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262";

    fn manifest(extra: &str) -> String {
        format!(
            "[corpus]\n\
             name = \"example\"\n\
             description = \"A corpus that exists to be parsed\"\n\
             source = \"https://example.invalid/example.parquet\"\n\
             licence = \"Apache-2.0\"\n\
             category = \"fetch\"\n\
             {extra}\n\
             [[files]]\n\
             path = \"example.parquet\"\n\
             blake3 = \"{EMPTY_DIGEST}\"\n\
             bytes = 1024\n"
        )
    }

    #[test]
    fn a_complete_manifest_parses() {
        let parsed = Manifest::parse(&manifest("")).unwrap();
        assert_eq!(parsed.corpus.name, "example");
        assert_eq!(parsed.corpus.category, Category::Fetch);
        assert_eq!(parsed.bytes(), 1024);
        assert!(parsed.assertions.rows.is_none());
    }

    #[test]
    fn assertions_are_read_when_they_are_there() {
        let text = format!(
            "{}\n[assertions]\nrows = 99997497\ncolumns = 105\n",
            manifest("")
        );
        let parsed = Manifest::parse(&text).unwrap();
        assert_eq!(parsed.assertions.rows, Some(99_997_497));
        assert_eq!(parsed.assertions.columns, Some(105));
    }

    #[test]
    fn a_mirrored_corpus_without_a_licence_note_is_refused() {
        let text = manifest("").replace("\"fetch\"", "\"mirror\"");
        let error = Manifest::parse(&text).unwrap_err();
        assert!(format!("{error}").contains("licence note"));
    }

    #[test]
    fn a_mirrored_corpus_with_a_licence_note_is_accepted() {
        let text = manifest("licence_note = \"Public domain, so redistribution is permitted\"")
            .replace("\"fetch\"", "\"mirror\"");
        assert_eq!(
            Manifest::parse(&text).unwrap().corpus.category,
            Category::Mirror
        );
    }

    /// A second file, so identity has something to be order independent about.
    const SECOND: &str = "\n[[files]]\npath = \"other.parquet\"\nblake3 = \
                          \"af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3263\"\n\
                          bytes = 2048\n";

    #[test]
    fn the_identity_does_not_move_when_entries_are_reordered() {
        let one = format!("{}{SECOND}", manifest(""));
        let two = {
            let base = manifest("");
            let (head, files) = base.split_at(base.find("[[files]]").unwrap());
            format!("{head}{}\n{files}", SECOND.trim_start())
        };
        assert_eq!(
            Manifest::parse(&one).unwrap().identity(),
            Manifest::parse(&two).unwrap().identity()
        );
    }

    #[test]
    fn the_identity_moves_when_the_corpus_does() {
        let base = Manifest::parse(&manifest("")).unwrap().identity();
        let more = Manifest::parse(&format!("{}{SECOND}", manifest("")))
            .unwrap()
            .identity();
        assert_ne!(base, more);

        // A different corpus made of the same bytes is a different corpus, because a result row
        // carrying only the identity still has to say which selection it was.
        let mut parsed = Manifest::parse(&manifest("")).unwrap();
        parsed.corpus.name = "example-36".to_owned();
        assert_ne!(parsed.identity(), base);
    }

    #[test]
    fn the_identity_does_not_move_when_a_comment_does() {
        // Deliberate. A rewritten comment is not a different corpus, and an identity that changed
        // for one would teach everybody to ignore the field.
        let commented = format!("# a note somebody added later\n{}", manifest(""));
        assert_eq!(
            Manifest::parse(&commented).unwrap().identity(),
            Manifest::parse(&manifest("")).unwrap().identity()
        );
    }

    #[test]
    fn a_corpus_that_says_it_is_part_of_itself_is_refused() {
        let text = manifest("part_of = \"example\"");
        let error = Manifest::parse(&text).unwrap_err();
        assert!(format!("{error}").contains("part of itself"));
    }

    #[test]
    fn a_part_has_to_be_some_of_the_whole_and_not_just_smaller() {
        let whole = Manifest::parse(&format!("{}{SECOND}", manifest(""))).unwrap();
        let part = Manifest::parse(&manifest("part_of = \"bigger\"")).unwrap();
        assert!(part.is_part_of(&whole));

        // Same path, different bytes. This is the drift the check exists for, and it is the one
        // that looks fine in a diff because the path is what a reader compares.
        let moved =
            Manifest::parse(&manifest("part_of = \"bigger\"").replace("1024", "2048")).unwrap();
        assert!(!moved.is_part_of(&whole));
    }

    #[test]
    fn a_part_the_same_size_as_the_whole_is_not_a_part() {
        let whole = Manifest::parse(&manifest("")).unwrap();
        let part = Manifest::parse(&manifest("part_of = \"bigger\"")).unwrap();
        assert!(!part.is_part_of(&whole));
    }

    /// A generate manifest, which needs a different source and a generator to go with it.
    fn generated(extra: &str) -> String {
        manifest("")
            .replace("\"fetch\"", "\"generate\"")
            .replace(
                "https://example.invalid/example.parquet",
                "example-gen 1.0.0 at scale factor 1",
            )
            .replace(
                "[[files]]",
                &format!(
                    "[generator]\n\
                     program = \"example-gen\"\n\
                     arguments = [\"-s\", \"1\"]\n\
                     version = \"1.0.0\"\n\
                     {extra}\n\
                     [[files]]"
                ),
            )
    }

    #[test]
    fn a_generated_corpus_says_what_produces_it() {
        let parsed = Manifest::parse(&generated("")).unwrap();
        let generator = parsed.generator.unwrap();
        assert_eq!(generator.program, "example-gen");
        assert_eq!(generator.arguments, ["-s", "1"]);
        // Not written in the manifest above, so this is the default arriving.
        assert_eq!(generator.version_arguments, ["-h"]);
    }

    #[test]
    fn a_generated_corpus_with_no_generator_is_refused() {
        let text = manifest("").replace("\"fetch\"", "\"generate\"");
        let error = Manifest::parse(&text).unwrap_err();
        assert!(format!("{error}").contains("nothing says how"));
    }

    #[test]
    fn a_fetched_corpus_with_a_generator_is_refused() {
        let text = generated("").replace("\"generate\"", "\"fetch\"");
        let error = Manifest::parse(&text).unwrap_err();
        assert!(format!("{error}").contains("two different answers"));
    }

    #[test]
    fn a_generator_that_cannot_be_asked_its_version_is_refused() {
        let text = generated("version_arguments = []");
        let error = Manifest::parse(&text).unwrap_err();
        assert!(format!("{error}").contains("cannot be checked"));
    }

    #[test]
    fn the_environment_a_generator_needs_is_read() {
        let text = generated("[generator.environment]\nOUT = \"{output}\"");
        let parsed = Manifest::parse(&text).unwrap();
        assert_eq!(
            parsed.generator.unwrap().environment.get("OUT").unwrap(),
            "{output}"
        );
    }

    #[test]
    fn a_manifest_that_pins_no_files_is_refused() {
        let text = manifest("");
        let (head, _) = text.split_once("[[files]]").unwrap();
        let error = Manifest::parse(head).unwrap_err();
        assert!(format!("{error}").contains("pins nothing"));
    }

    #[test]
    fn the_same_path_listed_twice_is_refused() {
        let text = manifest("");
        let (_, files) = text.split_once("[[files]]").unwrap();
        let error = Manifest::parse(&format!("{text}[[files]]{files}")).unwrap_err();
        assert!(format!("{error}").contains("listed twice"));
    }

    #[test]
    fn a_path_that_climbs_out_of_the_corpus_is_refused() {
        let text = manifest("").replace("\"example.parquet\"", "\"../../etc/passwd\"");
        let error = Manifest::parse(&text).unwrap_err();
        assert!(format!("{error}").contains("climbs out"));
    }

    #[test]
    fn a_file_declared_as_zero_bytes_is_refused() {
        let text = manifest("").replace("bytes = 1024", "bytes = 0");
        let error = Manifest::parse(&text).unwrap_err();
        assert!(format!("{error}").contains("pins nothing"));
    }

    #[test]
    fn a_field_this_format_does_not_have_is_refused_rather_than_ignored() {
        // A typo in a field name is otherwise a field that silently does nothing, and the field
        // most worth typoing is `licence_note`.
        let text = manifest("licence_notes = \"nearly right\"");
        let error = Manifest::parse(&text).unwrap_err();
        assert!(format!("{error}").contains("licence_notes"));
    }

    #[test]
    fn a_digest_that_is_not_a_digest_is_refused_at_parse_time() {
        let text = manifest("").replace(EMPTY_DIGEST, "not-a-digest");
        let error = Manifest::parse(&text).unwrap_err();
        assert!(matches!(error, ManifestError::Parse { .. }));
    }

    #[test]
    fn a_manifest_that_disagrees_with_its_directory_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let corpus = dir.path().join("something-else");
        std::fs::create_dir(&corpus).unwrap();
        let path = corpus.join("manifest.toml");
        std::fs::write(&path, manifest("")).unwrap();
        let error = Manifest::read(&path).unwrap_err();
        assert!(format!("{error}").contains("directory called something-else"));
    }

    #[test]
    fn a_manifest_that_agrees_with_its_directory_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let corpus = dir.path().join("example");
        std::fs::create_dir(&corpus).unwrap();
        let path = corpus.join("manifest.toml");
        std::fs::write(&path, manifest("")).unwrap();
        assert_eq!(Manifest::read(&path).unwrap().corpus.name, "example");
    }
}
