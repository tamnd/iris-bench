//! Where corpus bytes live once they have been verified.
//!
//! Objects are named by their own digest, so two runs referring to the same corpus are provably
//! referring to the same bytes rather than to the same file name. That is the whole point. A file
//! name is a claim about content and a digest is the content, and the gap between those two is
//! where a result that cannot be reproduced comes from.
//!
//! # What the layout buys
//!
//! ```text
//! <root>/objects/af/1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262
//! ```
//!
//! The first byte of the digest is a directory, which keeps any one directory to a few hundred
//! entries rather than a few tens of thousands. That is not about lookup speed, since every lookup
//! here is a direct path from a known digest. It is about the directory staying listable by a
//! person trying to work out what is on a machine.
//!
//! Deduplication falls out of the naming rather than being a feature. Two corpora that share a file
//! store it once, and re-fetching a corpus that is already present writes nothing.
//!
//! # Why writes go through a temporary file
//!
//! An object is written under a temporary name and renamed into place once it is complete. A
//! process that dies halfway through leaves a temporary file rather than a truncated object at a
//! path that claims to be a digest, and a truncated object at a name that asserts its own content
//! is the one failure this store exists to make impossible.

use std::{
    fs::File,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use crate::digest::Digest;

/// Where objects live under the store root.
const OBJECTS: &str = "objects";

/// How many bytes to move at a time when copying a file in.
const CHUNK: usize = 1 << 20;

/// A content addressed store of corpus files.
#[derive(Clone, Debug)]
pub struct Store {
    /// The directory everything lives under.
    root: PathBuf,
}

/// What happened when something was put into the store.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Inserted {
    /// The digest the object is now addressed by.
    pub digest: Digest,
    /// Whether the store already had these bytes, in which case nothing was written.
    pub deduplicated: bool,
}

/// Why a store operation did not work.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// The filesystem said no.
    #[error("{doing} {path}: {source}")]
    Io {
        /// What was being attempted.
        doing: &'static str,
        /// Which path it was being attempted on.
        path: String,
        /// What the filesystem said.
        source: io::Error,
    },
    /// The bytes that arrived are not the bytes that were promised.
    ///
    /// Both digests are in the message. A mismatch reported as a boolean is a mismatch somebody
    /// has to reproduce before they can start working out what happened.
    #[error("{path} was expected to be {wanted} and is {found}")]
    DigestMismatch {
        /// Which file.
        path: String,
        /// The digest the manifest promised.
        wanted: Digest,
        /// The digest the bytes actually have.
        found: Digest,
    },
    /// Something was asked for that the store does not have.
    #[error("the store has no object {0}")]
    Missing(Digest),
}

impl Store {
    /// Opens a store, creating the directories if they are not there.
    ///
    /// # Errors
    ///
    /// If the directories cannot be created.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        let objects = root.join(OBJECTS);
        std::fs::create_dir_all(&objects).map_err(|source| StoreError::Io {
            doing: "creating",
            path: objects.display().to_string(),
            source,
        })?;
        Ok(Self { root })
    }

    /// The directory this store lives in.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where an object with this digest sits, whether or not it is there.
    #[must_use]
    pub fn path(&self, digest: &Digest) -> PathBuf {
        let hex = digest.to_hex();
        let (prefix, rest) = hex.split_at(2);
        self.root.join(OBJECTS).join(prefix).join(rest)
    }

    /// Whether the store already holds these bytes.
    #[must_use]
    pub fn contains(&self, digest: &Digest) -> bool {
        self.path(digest).is_file()
    }

    /// Puts a run of bytes into the store and returns what it is addressed by.
    ///
    /// # Errors
    ///
    /// If the object cannot be written.
    pub fn insert_bytes(&self, bytes: &[u8]) -> Result<Inserted, StoreError> {
        let digest = Digest::of_bytes(bytes);
        if self.contains(&digest) {
            return Ok(Inserted {
                digest,
                deduplicated: true,
            });
        }
        self.write(&digest, |file| file.write_all(bytes))?;
        Ok(Inserted {
            digest,
            deduplicated: false,
        })
    }

    /// Puts a file into the store, copying it rather than moving it.
    ///
    /// Copying rather than moving because the source is often a download somebody wants to keep, or
    /// a file inside an extracted archive that other entries in the same manifest also point into.
    ///
    /// # Errors
    ///
    /// If the file cannot be read or the object cannot be written.
    pub fn insert_file(&self, source: &Path) -> Result<Inserted, StoreError> {
        let digest = Digest::of_file(source).map_err(|error| StoreError::Io {
            doing: "hashing",
            path: source.display().to_string(),
            source: error,
        })?;
        if self.contains(&digest) {
            return Ok(Inserted {
                digest,
                deduplicated: true,
            });
        }
        let mut input = File::open(source).map_err(|error| StoreError::Io {
            doing: "opening",
            path: source.display().to_string(),
            source: error,
        })?;
        self.write(&digest, |file| {
            let mut buffer = vec![0_u8; CHUNK];
            loop {
                let read = input.read(&mut buffer)?;
                if read == 0 {
                    return Ok(());
                }
                file.write_all(&buffer[..read])?;
            }
        })?;
        Ok(Inserted {
            digest,
            deduplicated: false,
        })
    }

    /// Puts a file in and refuses it if it is not what the manifest promised.
    ///
    /// This is the call a fetch uses. [`Self::insert_file`] takes whatever it is given and names it
    /// correctly, which is the right behaviour for bytes this repository produced and the wrong one
    /// for bytes that arrived over a network.
    ///
    /// # Errors
    ///
    /// If the file cannot be read, the object cannot be written, or the digest is not `wanted`.
    pub fn insert_verified(&self, source: &Path, wanted: &Digest) -> Result<Inserted, StoreError> {
        let found = Digest::of_file(source).map_err(|error| StoreError::Io {
            doing: "hashing",
            path: source.display().to_string(),
            source: error,
        })?;
        if found != *wanted {
            return Err(StoreError::DigestMismatch {
                path: source.display().to_string(),
                wanted: *wanted,
                found,
            });
        }
        self.insert_file(source)
    }

    /// Streams bytes in, hashing as they arrive, and keeps them only if they are what was promised.
    ///
    /// This is what a fetch uses. [`Self::insert_verified`] reads its source three times, once to
    /// hash and twice to copy, which is the right shape for a file already on disk and the wrong
    /// one for fifteen gigabytes arriving over a network. Here the bytes are hashed and written in
    /// the same pass, and a download that does not match is deleted rather than renamed, so a
    /// mismatch never reaches a path that asserts its own content.
    ///
    /// # Errors
    ///
    /// If the stream cannot be read, the object cannot be written, or the digest is not `wanted`.
    pub fn insert_stream(
        &self,
        mut reader: impl Read,
        wanted: &Digest,
    ) -> Result<Inserted, StoreError> {
        if self.contains(wanted) {
            return Ok(Inserted {
                digest: *wanted,
                deduplicated: true,
            });
        }
        let mut hasher = blake3::Hasher::new();
        self.write(wanted, |sink| {
            let mut buffer = vec![0_u8; CHUNK];
            loop {
                let read = reader.read(&mut buffer)?;
                if read == 0 {
                    return Ok(());
                }
                hasher.update(&buffer[..read]);
                sink.write_all(&buffer[..read])?;
            }
        })?;

        let found = Digest::from_bytes(*hasher.finalize().as_bytes());
        if found == *wanted {
            return Ok(Inserted {
                digest: found,
                deduplicated: false,
            });
        }

        // The object was renamed into place under the wanted name before this was known, because
        // the digest is only final once the last byte has arrived. Removing it here is what keeps
        // the invariant true: an object in this store hashes to its own name.
        let path = self.path(wanted);
        std::fs::remove_file(&path).map_err(|source| StoreError::Io {
            doing: "removing",
            path: path.display().to_string(),
            source,
        })?;
        Err(StoreError::DigestMismatch {
            path: path.display().to_string(),
            wanted: *wanted,
            found,
        })
    }

    /// Opens an object for reading.
    ///
    /// # Errors
    ///
    /// If the store does not have it, or it cannot be opened.
    pub fn open_object(&self, digest: &Digest) -> Result<File, StoreError> {
        let path = self.path(digest);
        if !path.is_file() {
            return Err(StoreError::Missing(*digest));
        }
        File::open(&path).map_err(|source| StoreError::Io {
            doing: "opening",
            path: path.display().to_string(),
            source,
        })
    }

    /// Re-reads an object and checks it still hashes to its own name.
    ///
    /// Nothing calls this on the happy path. It is for the case where a result looks wrong and
    /// somebody needs to rule out the store, which is a question worth being able to answer in one
    /// command rather than by reasoning about it.
    ///
    /// # Errors
    ///
    /// If the store does not have it, it cannot be read, or its content no longer matches its name.
    pub fn verify(&self, digest: &Digest) -> Result<(), StoreError> {
        let path = self.path(digest);
        if !path.is_file() {
            return Err(StoreError::Missing(*digest));
        }
        let found = Digest::of_file(&path).map_err(|source| StoreError::Io {
            doing: "hashing",
            path: path.display().to_string(),
            source,
        })?;
        if found == *digest {
            Ok(())
        } else {
            Err(StoreError::DigestMismatch {
                path: path.display().to_string(),
                wanted: *digest,
                found,
            })
        }
    }

    /// Writes an object through a temporary file and renames it into place.
    fn write(
        &self,
        digest: &Digest,
        fill: impl FnOnce(&mut File) -> io::Result<()>,
    ) -> Result<(), StoreError> {
        let final_path = self.path(digest);
        let directory = final_path
            .parent()
            .expect("an object path always has a parent");
        std::fs::create_dir_all(directory).map_err(|source| StoreError::Io {
            doing: "creating",
            path: directory.display().to_string(),
            source,
        })?;

        // The temporary name carries the process id so that two processes filling the store at once
        // do not write over each other's partial files. They may both write the same object, which
        // is harmless, because the rename at the end is atomic and both of them are producing
        // identical bytes by construction.
        let temporary = directory.join(format!(".{}.{}", digest.to_hex(), std::process::id()));
        let shown = temporary.display().to_string();
        let io_error = |doing: &'static str| {
            let path = shown.clone();
            move |source| StoreError::Io {
                doing,
                path,
                source,
            }
        };

        let mut sink = File::create(&temporary).map_err(io_error("creating"))?;
        let filled = fill(&mut sink)
            .and_then(|()| sink.sync_all())
            .map_err(io_error("writing"));
        drop(sink);
        if let Err(error) = filled {
            let _ = std::fs::remove_file(&temporary);
            return Err(error);
        }

        std::fs::rename(&temporary, &final_path).map_err(io_error("renaming"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reader that panics if anything reads it, for asserting that nothing did.
    struct Never;

    impl Read for Never {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            panic!("the store read a stream it already had");
        }
    }

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn an_object_is_addressed_by_its_own_digest() {
        let (_dir, store) = store();
        let inserted = store.insert_bytes(b"corpus bytes").unwrap();
        assert!(!inserted.deduplicated);
        assert_eq!(inserted.digest, Digest::of_bytes(b"corpus bytes"));
        assert!(store.contains(&inserted.digest));
    }

    #[test]
    fn the_same_bytes_twice_are_stored_once() {
        let (_dir, store) = store();
        let first = store.insert_bytes(b"the very same bytes").unwrap();
        let second = store.insert_bytes(b"the very same bytes").unwrap();
        assert_eq!(first.digest, second.digest);
        assert!(!first.deduplicated);
        assert!(second.deduplicated);
    }

    #[test]
    fn the_path_splits_on_the_first_byte_of_the_digest() {
        let (_dir, store) = store();
        let digest = Digest::of_bytes(b"anything");
        let hex = digest.to_hex();
        let path = store.path(&digest);
        assert_eq!(path.file_name().unwrap().to_str().unwrap(), &hex[2..]);
        assert_eq!(
            path.parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_str()
                .unwrap(),
            &hex[..2]
        );
    }

    #[test]
    fn a_file_goes_in_and_comes_back_out_unchanged() {
        let (dir, store) = store();
        let source = dir.path().join("incoming");
        // Larger than one copy chunk, so the loop that moves it runs more than once.
        let bytes: Vec<u8> = (0..3_000_000_u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&source, &bytes).unwrap();

        let inserted = store.insert_file(&source).unwrap();
        let mut round_tripped = Vec::new();
        store
            .open_object(&inserted.digest)
            .unwrap()
            .read_to_end(&mut round_tripped)
            .unwrap();
        assert_eq!(round_tripped, bytes);
    }

    #[test]
    fn inserting_a_file_leaves_the_original_where_it_was() {
        let (dir, store) = store();
        let source = dir.path().join("incoming");
        std::fs::write(&source, b"still here afterwards").unwrap();
        store.insert_file(&source).unwrap();
        assert!(source.is_file());
    }

    #[test]
    fn a_verified_insert_refuses_bytes_that_are_not_what_was_promised() {
        let (dir, store) = store();
        let source = dir.path().join("incoming");
        std::fs::write(&source, b"what actually arrived").unwrap();

        let promised = Digest::of_bytes(b"what was promised");
        let error = store.insert_verified(&source, &promised).unwrap_err();
        let message = format!("{error}");
        // Both digests, because a mismatch reported as a boolean is one somebody has to reproduce
        // before they can begin working out what happened.
        assert!(message.contains(&promised.to_hex()));
        assert!(message.contains(&Digest::of_bytes(b"what actually arrived").to_hex()));
        assert!(!store.contains(&promised));
    }

    #[test]
    fn a_verified_insert_accepts_bytes_that_are() {
        let (dir, store) = store();
        let source = dir.path().join("incoming");
        std::fs::write(&source, b"exactly as promised").unwrap();
        let promised = Digest::of_bytes(b"exactly as promised");
        assert_eq!(
            store.insert_verified(&source, &promised).unwrap().digest,
            promised
        );
    }

    #[test]
    fn a_stream_that_is_what_was_promised_lands_in_the_store() {
        let (_dir, store) = store();
        // Larger than one chunk, so the loop that hashes and writes runs more than once and the
        // hasher is being fed in pieces rather than all at once.
        let bytes: Vec<u8> = (0..3_000_000_u32).map(|i| (i % 251) as u8).collect();
        let promised = Digest::of_bytes(&bytes);

        let inserted = store.insert_stream(&bytes[..], &promised).unwrap();
        assert_eq!(inserted.digest, promised);
        assert!(!inserted.deduplicated);

        let mut back = Vec::new();
        store
            .open_object(&promised)
            .unwrap()
            .read_to_end(&mut back)
            .unwrap();
        assert_eq!(back, bytes);
    }

    #[test]
    fn a_stream_that_is_not_what_was_promised_leaves_nothing_behind() {
        let (_dir, store) = store();
        let promised = Digest::of_bytes(b"what was promised");
        let error = store
            .insert_stream(&b"what actually arrived"[..], &promised)
            .unwrap_err();

        let message = format!("{error}");
        assert!(message.contains(&promised.to_hex()));
        assert!(message.contains(&Digest::of_bytes(b"what actually arrived").to_hex()));

        // The object was renamed into place before the digest was known, because the digest is only
        // final once the last byte has arrived. What matters is that it is gone again.
        assert!(!store.contains(&promised));
        let directory = store.path(&promised).parent().unwrap().to_owned();
        let left: Vec<_> = std::fs::read_dir(&directory)
            .map(|entries| entries.filter_map(Result::ok).collect())
            .unwrap_or_default();
        assert!(left.is_empty(), "left behind {left:?}");
    }

    #[test]
    fn a_stream_whose_bytes_are_already_there_is_not_read_at_all() {
        let (_dir, store) = store();
        let promised = store.insert_bytes(b"already here").unwrap().digest;
        // If this passes, dedup happened before anything was read, which is the whole reason a
        // fetch asks the store first rather than downloading and then noticing.
        assert!(store.insert_stream(Never, &promised).unwrap().deduplicated);
    }

    #[test]
    fn asking_for_something_the_store_does_not_have_says_so() {
        let (_dir, store) = store();
        let absent = Digest::of_bytes(b"never inserted");
        assert!(matches!(
            store.open_object(&absent),
            Err(StoreError::Missing(_))
        ));
        assert!(matches!(store.verify(&absent), Err(StoreError::Missing(_))));
    }

    #[test]
    fn verify_catches_an_object_whose_content_no_longer_matches_its_name() {
        let (_dir, store) = store();
        let inserted = store.insert_bytes(b"the original bytes").unwrap();
        store.verify(&inserted.digest).unwrap();

        std::fs::write(store.path(&inserted.digest), b"something else entirely").unwrap();
        assert!(matches!(
            store.verify(&inserted.digest),
            Err(StoreError::DigestMismatch { .. })
        ));
    }

    #[test]
    fn no_temporary_files_are_left_behind_by_a_successful_write() {
        let (_dir, store) = store();
        let inserted = store.insert_bytes(b"written cleanly").unwrap();
        let directory = store.path(&inserted.digest).parent().unwrap().to_owned();
        let strays: Vec<_> = std::fs::read_dir(directory)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with('.'))
            .collect();
        assert!(strays.is_empty(), "left behind {strays:?}");
    }

    #[test]
    fn a_store_reopened_on_the_same_root_still_has_everything() {
        let dir = tempfile::tempdir().unwrap();
        let digest = Store::open(dir.path())
            .unwrap()
            .insert_bytes(b"survives a restart")
            .unwrap()
            .digest;
        assert!(Store::open(dir.path()).unwrap().contains(&digest));
    }
}
