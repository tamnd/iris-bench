//! Corpus manifests, fetching, generation and content addressed storage.
//!
//! Every corpus is pinned by a BLAKE3 digest, including the ones that are
//! generated rather than downloaded. A digest mismatch on fetch is a hard
//! failure that prints both digests, because a corpus that quietly changed is
//! worse than one that is missing.
//!
//! Nothing is implemented yet. See the milestone that owns this crate in
//! `docs/ROADMAP.md`.

/// The version of this crate, as reported by build metadata.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
