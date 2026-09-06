//! Producing a corpus locally and refusing it if it is not what was promised.
//!
//! The same contract as a fetch, with a generator where the network would be. The manifest pins the
//! digest of every file, the generator runs, and what it wrote is checked against the pin. A
//! generator that is deterministic given its arguments makes that pin meaningful, and one that is
//! not fails here rather than six weeks later.
//!
//! # Why the version is checked first
//!
//! The bytes a generator writes are a property of the generator, so the pin is only a pin if the
//! program is the one the manifest was written against. Checking the version costs one process
//! start. Not checking it costs a digest mismatch on twenty gigabytes of output with nothing in the
//! message to suggest that the cause is a build two releases along.
//!
//! # Where the program comes from
//!
//! Not from here. TPC's tools are downloaded by the person running the generation, under TPC's own
//! terms, and `docs/LICENSING.md` says why this repository neither ships them nor redistributes
//! what they produce.
//!
//! # Why the platform is checked first
//!
//! For the same reason as the version, and for one platform it is not hypothetical. `dbgen` opens
//! its output with `fopen(path, "w")`, which is text mode, and on Windows the C runtime turns every
//! newline in text mode output into a carriage return and a newline. The bytes differ by
//! construction there, so a manifest pinned from Unix output cannot be produced on Windows, and the
//! digest mismatch that would result says nothing about the platform being the reason. The
//! manifest names the platforms its digests came from and generating anywhere else is refused.

use std::{
    collections::BTreeMap,
    fs::File,
    io,
    path::{Path, PathBuf},
    process::Command,
};

use crate::{
    digest::Digest,
    manifest::{Category, Entry, Generator, Manifest},
    progress::{Counting, Progress},
    store::{Store, StoreError},
};

/// What generating did to one file.
#[derive(Clone, Debug)]
pub struct Generated {
    /// The path inside the corpus.
    pub path: String,
    /// What the file is addressed by, which is also what the manifest promised.
    pub digest: Digest,
    /// How many bytes it is.
    pub bytes: u64,
    /// Whether the store already had it, in which case nothing was generated.
    pub deduplicated: bool,
}

/// Why a corpus was not produced.
#[derive(Debug, thiserror::Error)]
pub enum GenerateError {
    /// The corpus is not one that is produced locally.
    #[error(
        "{name} is a {category} corpus, so there is nothing to generate. It is downloaded, by \
         `iris-bench corpus {name}`"
    )]
    NotGeneratable {
        /// Which corpus.
        name: String,
        /// What it is instead.
        category: Category,
    },
    /// This is not a platform the generator is known to write the pinned bytes on.
    #[error(
        "{name} is pinned from output generated on {platforms}, and this is {found}. The digests \
         in the manifest are of that output, so generating here would fail the digest check rather \
         than produce a corpus, and the message would say nothing about the platform being the \
         reason. If {found} writes the same bytes, run the generator by hand, compare, and add \
         {found} to the manifest"
    )]
    Platform {
        /// Which corpus.
        name: String,
        /// Where its bytes are known to reproduce, as a phrase.
        platforms: String,
        /// What this machine is, as `std::env::consts::OS` spells it.
        found: &'static str,
    },
    /// The program could not be started.
    #[error(
        "running {program}: {source}. The generator is not shipped with this repository and has \
         to be obtained separately, which docs/CORPORA.md explains"
    )]
    Missing {
        /// What was being run.
        program: String,
        /// What the operating system said.
        source: io::Error,
    },
    /// The program that is there is not the one this corpus was pinned against.
    #[error(
        "{program} says it is {found}, and this corpus was generated with {wanted}. A different \
         generator writes different bytes, so the digests below would not match and the message \
         would not say why"
    )]
    Version {
        /// What was being run.
        program: String,
        /// What the manifest pins.
        wanted: String,
        /// What the program said about itself, trimmed to its first line.
        found: String,
    },
    /// The program ran and gave up.
    #[error("{program} exited with {status}")]
    Failed {
        /// What was being run.
        program: String,
        /// How it ended.
        status: String,
    },
    /// The program succeeded and did not write a file the manifest names.
    #[error(
        "{program} reported success and did not write {path}. Either the manifest names a file \
         this generator does not produce or the arguments are not the ones it was written for"
    )]
    Absent {
        /// What was being run.
        program: String,
        /// Which file the manifest names.
        path: String,
    },
    /// What was written is not the size the manifest declared.
    #[error("{path} was expected to be {wanted} bytes and is {found}")]
    Size {
        /// Which file.
        path: String,
        /// What the manifest declared.
        wanted: u64,
        /// What was written.
        found: u64,
    },
    /// What was written is not the file the manifest pinned.
    #[error(
        "{path} was expected to be {wanted} and {program} wrote {found}. This is not overridable. \
         A generator that no longer produces the pinned bytes is producing a different corpus, and \
         a result measured on it is not comparable with one measured before it changed"
    )]
    Digest {
        /// Which file inside the corpus.
        path: String,
        /// What produced the bytes.
        program: String,
        /// The digest the manifest pinned.
        wanted: Digest,
        /// The digest of what was written.
        found: Digest,
    },
    /// The scratch directory the generator writes into could not be prepared or cleared.
    #[error("{doing} the scratch directory {path}: {source}")]
    Scratch {
        /// What was being attempted.
        doing: &'static str,
        /// Which directory.
        path: String,
        /// What went wrong underneath.
        source: io::Error,
    },
    /// The store refused a file for some reason other than the digest.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Runs a manifest's generator and takes in every file it names.
///
/// `program` overrides where the generator is found, for the usual case of a `dbgen` built in a
/// directory of its own rather than installed. `scratch` is where the generator is allowed to write
/// and has to have room for the whole corpus, because a generator writes what it writes and only
/// then can any of it be checked.
///
/// `watch` is called as each written file is read back into the store, often enough to show that
/// something is happening and rarely enough that it is not itself the slow part.
///
/// # Errors
///
/// If the corpus is not generated, the program is absent or is a different version, the run fails,
/// a named file is not written, or what is written is not the size or the digest the manifest
/// promised.
pub fn corpus(
    manifest: &Manifest,
    store: &Store,
    program: Option<&Path>,
    scratch: &Path,
    watch: &mut dyn FnMut(Progress<'_>),
) -> Result<Vec<Generated>, GenerateError> {
    if manifest.corpus.category != Category::Generate {
        return Err(GenerateError::NotGeneratable {
            name: manifest.corpus.name.clone(),
            category: manifest.corpus.category,
        });
    }
    // Checked by `Manifest::validate`, which every path into a manifest goes through. Treated as a
    // missing generator rather than unwrapped, because a panic here would be a worse way to say the
    // same thing.
    let Some(generator) = manifest.generator.as_ref() else {
        return Err(GenerateError::NotGeneratable {
            name: manifest.corpus.name.clone(),
            category: manifest.corpus.category,
        });
    };

    // Asked before the generator runs, because a corpus already on the machine costs nothing to
    // have again and generating it costs hours. This is the whole payoff of addressing by content.
    if manifest
        .files
        .iter()
        .all(|entry| store.contains(&entry.blake3))
    {
        return Ok(manifest
            .files
            .iter()
            .map(|entry| {
                watch(Progress {
                    path: &entry.path,
                    done: entry.bytes,
                    total: entry.bytes,
                });
                Generated {
                    path: entry.path.clone(),
                    digest: entry.blake3,
                    bytes: entry.bytes,
                    deduplicated: true,
                }
            })
            .collect());
    }

    // After the store is asked and before anything is run. A corpus already on the machine is
    // bytes, and bytes do not care what wrote them or where, so a machine that cannot generate this
    // corpus can still use one somebody else generated. What it cannot do is make it here.
    if !generator
        .platforms
        .iter()
        .any(|platform| platform == std::env::consts::OS)
    {
        return Err(GenerateError::Platform {
            name: manifest.corpus.name.clone(),
            platforms: listed(&generator.platforms),
            found: std::env::consts::OS,
        });
    }

    let program = program.map_or_else(|| PathBuf::from(&generator.program), Path::to_path_buf);
    check_version(generator, &program)?;
    prepare(scratch)?;
    run(generator, &program, scratch)?;

    let mut generated = Vec::with_capacity(manifest.files.len());
    for entry in &manifest.files {
        generated.push(one(&program, entry, scratch, store, watch)?);
    }
    Ok(generated)
}

/// Names a list of platforms the way somebody would say it out loud.
fn listed(platforms: &[String]) -> String {
    match platforms {
        [] => "nowhere".to_owned(),
        [only] => only.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// Asks the program what it is and refuses to go on if the answer is not what the manifest pins.
fn check_version(generator: &Generator, program: &Path) -> Result<(), GenerateError> {
    let output = Command::new(program)
        .args(&generator.version_arguments)
        .output()
        .map_err(|source| GenerateError::Missing {
            program: program.display().to_string(),
            source,
        })?;

    // Both streams, and the exit status is ignored on purpose. A program asked to print its usage
    // commonly writes it to stderr and exits non zero, and that is a program answering the question
    // rather than a program failing.
    let mut said = String::from_utf8_lossy(&output.stdout).into_owned();
    said.push_str(&String::from_utf8_lossy(&output.stderr));
    if said.contains(generator.version.trim()) {
        return Ok(());
    }
    Err(GenerateError::Version {
        program: program.display().to_string(),
        wanted: generator.version.clone(),
        found: said.lines().next().unwrap_or("nothing").trim().to_owned(),
    })
}

/// Empties the scratch directory, or makes it.
///
/// Emptied rather than reused, because a generator that writes only some of its output leaves the
/// rest of a previous run in place, and a file from a previous run has every chance of matching its
/// digest. That is the one way this could report a corpus it did not produce.
fn prepare(scratch: &Path) -> Result<(), GenerateError> {
    let shown = scratch.display().to_string();
    if scratch.exists() {
        std::fs::remove_dir_all(scratch).map_err(|source| GenerateError::Scratch {
            doing: "clearing",
            path: shown.clone(),
            source,
        })?;
    }
    std::fs::create_dir_all(scratch).map_err(|source| GenerateError::Scratch {
        doing: "creating",
        path: shown,
        source,
    })
}

/// Runs the generator with its working directory and environment pointed at the scratch directory.
fn run(generator: &Generator, program: &Path, scratch: &Path) -> Result<(), GenerateError> {
    let status = Command::new(program)
        .args(&generator.arguments)
        .current_dir(scratch)
        .envs(environment(generator, program, scratch))
        .status()
        .map_err(|source| GenerateError::Missing {
            program: program.display().to_string(),
            source,
        })?;
    if status.success() {
        return Ok(());
    }
    Err(GenerateError::Failed {
        program: program.display().to_string(),
        status: status.to_string(),
    })
}

/// The environment the manifest asks for, with its two placeholders filled in.
///
/// `dbgen` needs one variable saying where to write and another saying where its distribution file
/// is, and those are facts about `dbgen` rather than about generators. They belong in the manifest
/// that names `dbgen`, which is why this substitutes rather than knowing.
fn environment(generator: &Generator, program: &Path, scratch: &Path) -> BTreeMap<String, String> {
    let directory = program
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    generator
        .environment
        .iter()
        .map(|(name, value)| {
            let filled = value
                .replace("{output}", &scratch.display().to_string())
                .replace("{program_directory}", &directory.display().to_string());
            (name.clone(), filled)
        })
        .collect()
}

/// Takes one written file into the store, checking it against what the manifest pinned.
fn one(
    program: &Path,
    entry: &Entry,
    scratch: &Path,
    store: &Store,
    watch: &mut dyn FnMut(Progress<'_>),
) -> Result<Generated, GenerateError> {
    let written = scratch.join(&entry.path);
    let file = File::open(&written).map_err(|_| GenerateError::Absent {
        program: program.display().to_string(),
        path: entry.path.clone(),
    })?;

    let mut counting = Counting::new(file, &entry.path, entry.bytes, watch);
    let inserted = store.insert_stream(&mut counting, &entry.blake3);
    let found = counting.seen;

    // Size before digest, for the same reason a fetch reports them apart. Here a wrong length says
    // the generator was given different arguments and a full length mismatch says it is a different
    // generator, and those send somebody to look in different places.
    if found != entry.bytes {
        return Err(GenerateError::Size {
            path: entry.path.clone(),
            wanted: entry.bytes,
            found,
        });
    }

    let inserted = match inserted {
        Ok(inserted) => inserted,
        Err(StoreError::DigestMismatch { wanted, found, .. }) => {
            return Err(GenerateError::Digest {
                path: entry.path.clone(),
                program: program.display().to_string(),
                wanted,
                found,
            });
        }
        Err(other) => return Err(other.into()),
    };
    Ok(Generated {
        path: entry.path.clone(),
        digest: inserted.digest,
        bytes: found,
        deduplicated: inserted.deduplicated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A generator written as a shell script, so these tests exercise the real process machinery
    /// rather than a stand in for it. Everything here is about what happens around a generator, and
    /// what a generator is has to stay outside this repository.
    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    /// A manifest for a corpus of one file holding `contents`.
    fn manifest(contents: &str) -> Manifest {
        let digest = Digest::of_bytes(contents.as_bytes());
        Manifest::parse(&format!(
            "[corpus]\n\
             name = \"example\"\n\
             description = \"A corpus produced by a shell script\"\n\
             source = \"example-gen 1.0.0\"\n\
             licence = \"Apache-2.0\"\n\
             category = \"generate\"\n\
             \n\
             [generator]\n\
             program = \"example-gen\"\n\
             version = \"1.0.0\"\n\
             platforms = [\"linux\", \"macos\", \"windows\"]\n\
             \n\
             [[files]]\n\
             path = \"table.tbl\"\n\
             blake3 = \"{}\"\n\
             bytes = {}\n",
            digest.to_hex(),
            contents.len()
        ))
        .unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn a_generator_that_writes_what_was_pinned_fills_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let program = script(
            dir.path(),
            "example-gen",
            "if [ \"$1\" = -h ]; then echo 1.0.0 >&2; exit 1; fi\nprintf 'a|b|\\n' > table.tbl",
        );
        let store = Store::open(dir.path().join("store")).unwrap();

        let generated = corpus(
            &manifest("a|b|\n"),
            &store,
            Some(&program),
            &dir.path().join("scratch"),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(generated.len(), 1);
        assert!(!generated[0].deduplicated);
        assert!(store.contains(&generated[0].digest));
    }

    #[cfg(unix)]
    #[test]
    fn a_generator_that_writes_something_else_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let program = script(
            dir.path(),
            "example-gen",
            "if [ \"$1\" = -h ]; then echo 1.0.0 >&2; exit 1; fi\nprintf 'x|y|\\n' > table.tbl",
        );
        let store = Store::open(dir.path().join("store")).unwrap();

        let error = corpus(
            &manifest("a|b|\n"),
            &store,
            Some(&program),
            &dir.path().join("scratch"),
            &mut |_| {},
        )
        .unwrap_err();
        // Same length, different bytes, so this has to come out as a digest rather than a size.
        assert!(matches!(error, GenerateError::Digest { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn a_generator_of_the_wrong_version_is_refused_before_it_runs() {
        let dir = tempfile::tempdir().unwrap();
        let program = script(
            dir.path(),
            "example-gen",
            "if [ \"$1\" = -h ]; then echo 2.0.0 >&2; exit 1; fi\nprintf 'a|b|\\n' > table.tbl",
        );
        let store = Store::open(dir.path().join("store")).unwrap();
        let scratch = dir.path().join("scratch");

        let error = corpus(
            &manifest("a|b|\n"),
            &store,
            Some(&program),
            &scratch,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(matches!(error, GenerateError::Version { .. }));
        // Before it runs, and not partway through: nothing was written and nothing was cleared.
        assert!(!scratch.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_generator_that_does_not_write_the_named_file_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let program = script(
            dir.path(),
            "example-gen",
            "if [ \"$1\" = -h ]; then echo 1.0.0 >&2; exit 1; fi\nprintf 'a|b|\\n' > other.tbl",
        );
        let store = Store::open(dir.path().join("store")).unwrap();

        let error = corpus(
            &manifest("a|b|\n"),
            &store,
            Some(&program),
            &dir.path().join("scratch"),
            &mut |_| {},
        )
        .unwrap_err();
        assert!(matches!(error, GenerateError::Absent { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn a_generator_that_fails_is_not_asked_for_its_output() {
        let dir = tempfile::tempdir().unwrap();
        let program = script(
            dir.path(),
            "example-gen",
            "if [ \"$1\" = -h ]; then echo 1.0.0 >&2; exit 1; fi\nexit 3",
        );
        let store = Store::open(dir.path().join("store")).unwrap();

        let error = corpus(
            &manifest("a|b|\n"),
            &store,
            Some(&program),
            &dir.path().join("scratch"),
            &mut |_| {},
        )
        .unwrap_err();
        assert!(matches!(error, GenerateError::Failed { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn a_corpus_already_in_the_store_is_not_generated_again() {
        let dir = tempfile::tempdir().unwrap();
        // Refuses to run at all. If this passes, the generator was never started.
        let program = script(dir.path(), "example-gen", "exit 9");
        let store = Store::open(dir.path().join("store")).unwrap();
        store.insert_bytes(b"a|b|\n").unwrap();

        let generated = corpus(
            &manifest("a|b|\n"),
            &store,
            Some(&program),
            &dir.path().join("scratch"),
            &mut |_| {},
        )
        .unwrap();
        assert!(generated[0].deduplicated);
    }

    #[cfg(unix)]
    #[test]
    fn what_a_previous_run_left_behind_is_cleared_before_the_generator_runs() {
        let dir = tempfile::tempdir().unwrap();
        let scratch = dir.path().join("scratch");
        std::fs::create_dir_all(&scratch).unwrap();
        std::fs::write(scratch.join("table.tbl"), "a|b|\n").unwrap();

        // Writes nothing at all, so the only way `table.tbl` could be there is the stale copy.
        let program = script(
            dir.path(),
            "example-gen",
            "if [ \"$1\" = -h ]; then echo 1.0.0 >&2; exit 1; fi\ntrue",
        );
        let store = Store::open(dir.path().join("store")).unwrap();

        let error = corpus(
            &manifest("a|b|\n"),
            &store,
            Some(&program),
            &scratch,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(matches!(error, GenerateError::Absent { .. }));
    }

    #[test]
    fn a_fetched_corpus_is_not_generated() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let mut fetched = manifest("a|b|\n");
        fetched.corpus.category = Category::Fetch;
        fetched.generator = None;

        let error = corpus(&fetched, &store, None, dir.path(), &mut |_| {}).unwrap_err();
        assert!(matches!(error, GenerateError::NotGeneratable { .. }));
    }

    #[test]
    fn a_program_that_is_not_there_says_where_to_get_one() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("store")).unwrap();

        let error = corpus(
            &manifest("a|b|\n"),
            &store,
            Some(Path::new("no-such-generator-anywhere")),
            &dir.path().join("scratch"),
            &mut |_| {},
        )
        .unwrap_err();
        assert!(format!("{error}").contains("obtained separately"));
    }

    #[test]
    fn the_placeholders_a_manifest_may_use_are_filled_in() {
        let generated = manifest("a|b|\n");
        let mut generator = generated.generator.unwrap();
        generator
            .environment
            .insert("OUT".to_owned(), "{output}".to_owned());
        generator
            .environment
            .insert("CFG".to_owned(), "{program_directory}/dists.dss".to_owned());

        let filled = environment(
            &generator,
            Path::new("/opt/tpch/dbgen"),
            Path::new("/var/tmp/scratch"),
        );
        assert_eq!(filled["OUT"], "/var/tmp/scratch");
        assert_eq!(filled["CFG"], "/opt/tpch/dists.dss");
    }

    #[test]
    fn a_platform_the_corpus_was_not_pinned_on_is_refused_before_anything_runs() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("store")).unwrap();

        let mut pinned = manifest("a|b|\n");
        pinned.generator.as_mut().unwrap().platforms = vec!["plan9".to_owned()];
        // A program that does not exist, so this coming out as a platform refusal rather than as a
        // missing program is what says the platform was checked before anything was run.
        let error = corpus(
            &pinned,
            &store,
            Some(Path::new("nothing-that-exists")),
            &dir.path().join("scratch"),
            &mut |_| {},
        )
        .unwrap_err();

        assert!(matches!(error, GenerateError::Platform { .. }));
        assert!(format!("{error}").contains("plan9"));
        assert!(format!("{error}").contains(std::env::consts::OS));
    }

    #[test]
    fn a_corpus_already_in_the_store_is_not_refused_for_the_platform() {
        // Bytes do not care what wrote them or where. A machine that cannot generate this corpus
        // can still use one somebody else generated, and only the making of it is refused.
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("store")).unwrap();
        store.insert_bytes(b"a|b|\n").unwrap();

        let mut pinned = manifest("a|b|\n");
        pinned.generator.as_mut().unwrap().platforms = vec!["plan9".to_owned()];
        let generated = corpus(
            &pinned,
            &store,
            Some(Path::new("nothing-that-exists")),
            &dir.path().join("scratch"),
            &mut |_| {},
        )
        .unwrap();

        assert!(generated[0].deduplicated);
    }

    #[test]
    fn a_list_of_platforms_reads_as_a_sentence() {
        assert_eq!(listed(&["linux".to_owned()]), "linux");
        assert_eq!(
            listed(&["linux".to_owned(), "macos".to_owned()]),
            "linux and macos"
        );
        assert_eq!(
            listed(&["linux".to_owned(), "macos".to_owned(), "windows".to_owned()]),
            "linux, macos and windows"
        );
    }

    #[test]
    fn a_program_with_no_directory_resolves_its_directory_to_here() {
        let generated = manifest("a|b|\n");
        let mut generator = generated.generator.unwrap();
        generator
            .environment
            .insert("CFG".to_owned(), "{program_directory}".to_owned());

        let filled = environment(&generator, Path::new("dbgen"), Path::new("scratch"));
        assert_eq!(filled["CFG"], ".");
    }
}
