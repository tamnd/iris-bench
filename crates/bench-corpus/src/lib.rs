//! Corpus manifests, fetching, generation and content addressed storage.
//!
//! Every corpus is pinned by a BLAKE3 digest, including the ones that are generated rather than
//! downloaded. A digest mismatch on fetch is a hard failure that prints both digests, because a
//! corpus that quietly changed is worse than one that is missing.
//!
//! # The two halves
//!
//! A [`Manifest`] says what a corpus is: where it comes from, what licence it arrives under, which
//! of the three handling categories in `docs/LICENSING.md` applies, and the digest of every file.
//! The format is documented in `docs/CORPORA.md` and validated by `ci/discipline.py` for every
//! manifest committed to this repository.
//!
//! A [`Store`] holds the bytes, named by their own digest. Two runs referring to the same corpus
//! are then referring to the same bytes rather than to the same file name, which is the property
//! that makes a result worth re-running.
//!
//! Fetching and generation are not implemented yet. See B1 in `docs/ROADMAP.md`.

mod digest;
mod manifest;
mod store;

pub use digest::{Digest, DigestError};
pub use manifest::{Assertions, Category, Corpus, Entry, Manifest, ManifestError};
pub use store::{Inserted, Store, StoreError};

/// The version of this crate, as reported by build metadata.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
