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
//! # What arriving guarantees
//!
//! [`fetch::corpus`] downloads what a manifest names and hashes it in the same pass that writes it,
//! so bytes that are not what was promised never reach the store. [`generate::corpus`] does the same
//! for a corpus produced locally, with a generator where the network would be, and checks the
//! generator's version first because the bytes a generator writes are a property of the generator.
//! [`shape::check`] then reads the Parquet footers and refuses a corpus that is not the row and
//! column count the manifest asserted, which is the check that catches a download that stopped early
//! and still parses.

mod digest;
pub mod fetch;
pub mod generate;
mod manifest;
mod progress;
pub mod shape;
mod store;

pub use digest::{Digest, DigestError};
pub use fetch::{FetchError, Fetched};
pub use generate::{GenerateError, Generated};
pub use manifest::{Assertions, Category, Corpus, Entry, Generator, Manifest, ManifestError};
pub use progress::Progress;
pub use shape::{Shape, ShapeError};
pub use store::{Inserted, Store, StoreError};

/// The version of this crate, as reported by build metadata.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
