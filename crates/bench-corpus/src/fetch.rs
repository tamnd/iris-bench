//! Getting a corpus onto the machine and refusing it if it is not what was promised.
//!
//! A fetch is not a download. A download gets bytes, and a fetch gets the bytes a manifest named,
//! or it fails. The digest is checked in the same pass that writes the file, so a corpus that
//! quietly changed at its source never reaches the store at all.
//!
//! # Where a file comes from
//!
//! An entry may carry its own `url`. When it does not, the file's URL is its `path` resolved
//! against the corpus `source`, which is ordinary URL resolution: everything after the last slash
//! of `source` is replaced by `path`. For a single file corpus whose source is the file itself,
//! that resolves to the source unchanged, which is the common case and needs nothing written down.
//! For a corpus of many files under one directory, `source` names any one of them, or the directory
//! with a trailing slash, and each entry's path is appended.

use std::{
    io::{self, Read},
    time::Duration,
};

use crate::{
    digest::Digest,
    manifest::{Category, Entry, Manifest},
    store::{Store, StoreError},
};

/// How long to wait for the far end to start answering.
///
/// There is no limit on the transfer itself. A corpus here is tens of gigabytes and the honest
/// upper bound on how long that takes on an unknown link is not a number anybody can write down, so
/// the timeout that exists is the one that catches a host which is not answering at all.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// How often to tell the caller how far along a download is, in bytes.
const REPORT_EVERY: u64 = 64 << 20;

/// What a fetch did to one file.
#[derive(Clone, Debug)]
pub struct Fetched {
    /// The path inside the corpus.
    pub path: String,
    /// What the file is addressed by, which is also what the manifest promised.
    pub digest: Digest,
    /// How many bytes it is.
    pub bytes: u64,
    /// Whether the store already had it, in which case nothing was downloaded.
    pub deduplicated: bool,
}

/// How far along one file is.
#[derive(Clone, Copy, Debug)]
pub struct Progress<'a> {
    /// The path inside the corpus.
    pub path: &'a str,
    /// How many bytes have arrived.
    pub done: u64,
    /// How many the manifest says there are.
    pub total: u64,
}

/// Why a corpus did not arrive.
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    /// The corpus is not one that can be downloaded.
    #[error(
        "{name} is a {category} corpus, so there is nothing to fetch. A generated corpus is \
         produced locally and a mirrored one is already in the tree"
    )]
    NotFetchable {
        /// Which corpus.
        name: String,
        /// What it is instead.
        category: Category,
    },
    /// The corpus source is not a URL and no entry says where its bytes are.
    #[error(
        "{name}: {path} has no url and the corpus source is {declared}, which is not one either. A \
         fetched corpus needs somewhere to fetch from"
    )]
    NoUrl {
        /// Which corpus.
        name: String,
        /// Which file.
        path: String,
        /// What the corpus source says.
        declared: String,
    },
    /// The request did not get through.
    #[error("requesting {url}: {source}")]
    Request {
        /// Which URL.
        url: String,
        /// What the HTTP client said.
        source: Box<ureq::Error>,
    },
    /// The far end answered, and answered no.
    #[error("{url} answered {status}")]
    Status {
        /// Which URL.
        url: String,
        /// What it answered.
        status: u16,
    },
    /// The transfer stopped partway.
    #[error("reading {url}: {source}")]
    Transfer {
        /// Which URL.
        url: String,
        /// What went wrong underneath.
        source: io::Error,
    },
    /// What arrived is not the size the manifest declared.
    ///
    /// Checked separately from the digest even though the digest would catch it, because a wrong
    /// length says truncated download and a wrong digest says different file, and those two send
    /// somebody to look in completely different places.
    #[error("{path} was expected to be {wanted} bytes and is {found}")]
    Size {
        /// Which file.
        path: String,
        /// What the manifest declared.
        wanted: u64,
        /// What arrived.
        found: u64,
    },
    /// What arrived is not the file the manifest pinned.
    ///
    /// Reported here rather than passed through from the store, because the store's version of this
    /// names the path the bytes briefly occupied, which reads like there is a corrupt object sitting
    /// in the store when the whole point is that there is not. What somebody needs is the URL.
    #[error(
        "{path} was expected to be {wanted} and what {url} served is {found}. This is not \
         overridable. A corpus that changed at its source is a different corpus, and a result \
         measured on it is not comparable with one measured before it changed"
    )]
    Digest {
        /// Which file inside the corpus.
        path: String,
        /// Where the bytes came from.
        url: String,
        /// The digest the manifest pinned.
        wanted: Digest,
        /// The digest of what arrived.
        found: Digest,
    },
    /// The store refused it for some reason other than the digest.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Where one entry's bytes are.
///
/// # Errors
///
/// If the entry has no url of its own and the corpus source is not a URL either.
pub fn url_for(manifest: &Manifest, entry: &Entry) -> Result<String, FetchError> {
    if let Some(url) = entry.url.as_ref().filter(|url| !url.trim().is_empty()) {
        return Ok(url.clone());
    }
    // Ordinary relative URL resolution against a base, spelled out rather than pulled in. Splitting
    // on the scheme separator first is what keeps the slashes in `https://` from being mistaken for
    // the last slash of a path.
    let Some((scheme, rest)) = manifest
        .corpus
        .source
        .trim()
        .split_once("://")
        .filter(|(scheme, _)| *scheme == "http" || *scheme == "https")
    else {
        return Err(FetchError::NoUrl {
            name: manifest.corpus.name.clone(),
            path: entry.path.clone(),
            declared: manifest.corpus.source.clone(),
        });
    };
    let base = match rest.rfind('/') {
        Some(cut) => &rest[..=cut],
        None => return Ok(format!("{scheme}://{rest}/{}", entry.path)),
    };
    Ok(format!("{scheme}://{base}{}", entry.path))
}

/// Downloads every file a manifest names that the store does not already have.
///
/// `watch` is called as bytes arrive, often enough to show that something is happening and rarely
/// enough that it is not itself the slow part.
///
/// # Errors
///
/// If the corpus is not fetchable, a URL cannot be worked out, a request fails, a transfer stops
/// partway, or what arrives is not the size or the digest the manifest promised.
pub fn corpus(
    manifest: &Manifest,
    store: &Store,
    watch: &mut dyn FnMut(Progress<'_>),
) -> Result<Vec<Fetched>, FetchError> {
    if manifest.corpus.category != Category::Fetch {
        return Err(FetchError::NotFetchable {
            name: manifest.corpus.name.clone(),
            category: manifest.corpus.category,
        });
    }

    let mut client = Client::new();
    let mut fetched = Vec::with_capacity(manifest.files.len());
    for entry in &manifest.files {
        fetched.push(one(&mut client, manifest, entry, store, watch)?);
    }
    Ok(fetched)
}

/// The HTTP client, and the one piece of network reality it has to cope with.
///
/// A machine can have a default IPv6 route that does not work. The name resolves to an AAAA record,
/// the connection fails with network unreachable, and every browser and curl on that machine is
/// fine because they try the other family. This does the same thing, once, and then remembers.
///
/// Forcing IPv4 everywhere would be the shorter fix and the wrong one, since it would break the
/// machines where IPv6 is the family that works. This is not covered by a test, because what it
/// handles is a property of a network rather than of this code.
struct Client {
    /// Whichever family resolves first, which is what should normally be used.
    any: ureq::Agent,
    /// The fallback, built once and only when something has already failed.
    ipv4: Option<ureq::Agent>,
    /// Set once the fallback has worked, so the rest of a corpus does not pay for the failure.
    fallen_back: bool,
}

impl Client {
    /// Builds the client with no fallback yet.
    fn new() -> Self {
        Self {
            any: agent(ureq::config::IpFamily::Any),
            ipv4: None,
            fallen_back: false,
        }
    }

    /// Gets a URL, trying the other address family if the first attempt cannot connect at all.
    fn get(&mut self, url: &str) -> Result<ureq::http::Response<ureq::Body>, FetchError> {
        if !self.fallen_back {
            match self.any.get(url).call() {
                Ok(response) => return Ok(response),
                // A status code is an answer. Asking the same question over a different address
                // family would get the same answer and waste a request finding that out.
                Err(ureq::Error::StatusCode(status)) => {
                    return Err(FetchError::Status {
                        url: url.to_owned(),
                        status,
                    });
                }
                Err(first) => {
                    let ipv4 = self
                        .ipv4
                        .get_or_insert_with(|| agent(ureq::config::IpFamily::Ipv4Only));
                    let Ok(response) = ipv4.get(url).call() else {
                        // The first error is the one reported. The retry existing is an
                        // implementation detail and its error would only be confusing.
                        return Err(FetchError::Request {
                            url: url.to_owned(),
                            source: Box::new(first),
                        });
                    };
                    self.fallen_back = true;
                    return Ok(response);
                }
            }
        }

        let ipv4 = self
            .ipv4
            .get_or_insert_with(|| agent(ureq::config::IpFamily::Ipv4Only));
        ipv4.get(url).call().map_err(|source| match source {
            ureq::Error::StatusCode(status) => FetchError::Status {
                url: url.to_owned(),
                status,
            },
            other => FetchError::Request {
                url: url.to_owned(),
                source: Box::new(other),
            },
        })
    }
}

/// An agent restricted to one address family.
fn agent(family: ureq::config::IpFamily) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .ip_family(family)
        .build()
        .into()
}

/// Downloads one entry.
fn one(
    client: &mut Client,
    manifest: &Manifest,
    entry: &Entry,
    store: &Store,
    watch: &mut dyn FnMut(Progress<'_>),
) -> Result<Fetched, FetchError> {
    // Asked before the request is made, because the whole point of addressing by content is that a
    // corpus already on the machine costs nothing to have again.
    if store.contains(&entry.blake3) {
        watch(Progress {
            path: &entry.path,
            done: entry.bytes,
            total: entry.bytes,
        });
        return Ok(Fetched {
            path: entry.path.clone(),
            digest: entry.blake3,
            bytes: entry.bytes,
            deduplicated: true,
        });
    }

    let url = url_for(manifest, entry)?;
    let response = client.get(&url)?;

    let mut counting = Counting {
        inner: response.into_body().into_reader(),
        seen: 0,
        next: REPORT_EVERY,
        path: &entry.path,
        total: entry.bytes,
        watch,
    };
    let inserted = store.insert_stream(&mut counting, &entry.blake3);
    let found = counting.seen;

    // A wrong length and a wrong digest are the same event as far as the store is concerned, and it
    // has already refused and removed whatever arrived either way. Which of the two gets reported
    // matters to the person reading it, though: a short read is a truncated download and a full
    // length mismatch is a different file, and those send somebody to look in different places.
    if found != entry.bytes {
        return Err(FetchError::Size {
            path: entry.path.clone(),
            wanted: entry.bytes,
            found,
        });
    }

    let inserted = match inserted {
        Ok(inserted) => inserted,
        Err(StoreError::DigestMismatch { wanted, found, .. }) => {
            return Err(FetchError::Digest {
                path: entry.path.clone(),
                url,
                wanted,
                found,
            });
        }
        Err(other) => return Err(other.into()),
    };
    Ok(Fetched {
        path: entry.path.clone(),
        digest: inserted.digest,
        bytes: found,
        deduplicated: inserted.deduplicated,
    })
}

/// A reader that counts what goes through it and says so every so often.
struct Counting<'a, R> {
    /// The reader the bytes are actually coming from.
    inner: R,
    /// How many have gone through.
    seen: u64,
    /// The count at which to report next.
    next: u64,
    /// Which file, for the report.
    path: &'a str,
    /// How many bytes the manifest says there are, for the report.
    total: u64,
    /// Who to tell.
    watch: &'a mut dyn FnMut(Progress<'_>),
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

    const EMPTY_DIGEST: &str = "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262";

    fn manifest(source: &str, files: &str) -> Manifest {
        Manifest::parse(&format!(
            "[corpus]\n\
             name = \"example\"\n\
             description = \"A corpus that exists to have its URLs worked out\"\n\
             source = \"{source}\"\n\
             licence = \"Apache-2.0\"\n\
             category = \"fetch\"\n\
             {files}"
        ))
        .unwrap()
    }

    fn file(path: &str, url: &str) -> String {
        format!("[[files]]\npath = \"{path}\"\nblake3 = \"{EMPTY_DIGEST}\"\nbytes = 1024\n{url}\n")
    }

    #[test]
    fn a_single_file_source_resolves_to_itself() {
        let manifest = manifest(
            "https://example.invalid/sets/hits.parquet",
            &file("hits.parquet", ""),
        );
        assert_eq!(
            url_for(&manifest, &manifest.files[0]).unwrap(),
            "https://example.invalid/sets/hits.parquet"
        );
    }

    #[test]
    fn a_path_resolves_against_the_last_slash_of_the_source() {
        let manifest = manifest(
            "https://example.invalid/sets/hits.parquet",
            &file("part-1.parquet", ""),
        );
        assert_eq!(
            url_for(&manifest, &manifest.files[0]).unwrap(),
            "https://example.invalid/sets/part-1.parquet"
        );
    }

    #[test]
    fn a_source_ending_in_a_slash_is_a_directory() {
        let manifest = manifest("https://example.invalid/sets/", &file("part-1.parquet", ""));
        assert_eq!(
            url_for(&manifest, &manifest.files[0]).unwrap(),
            "https://example.invalid/sets/part-1.parquet"
        );
    }

    #[test]
    fn an_entry_with_its_own_url_uses_it() {
        let manifest = manifest(
            "https://example.invalid/sets/hits.parquet",
            &file(
                "part-1.parquet",
                "url = \"https://elsewhere.invalid/mirror/part-1.parquet\"",
            ),
        );
        assert_eq!(
            url_for(&manifest, &manifest.files[0]).unwrap(),
            "https://elsewhere.invalid/mirror/part-1.parquet"
        );
    }

    #[test]
    fn a_source_that_is_not_a_url_and_an_entry_that_does_not_say_is_an_error() {
        let manifest = manifest("tpch-dbgen -s 20", &file("lineitem.parquet", ""));
        let error = url_for(&manifest, &manifest.files[0]).unwrap_err();
        assert!(matches!(error, FetchError::NoUrl { .. }));
    }

    #[test]
    fn a_generated_corpus_is_not_fetched() {
        let mut generated = manifest(
            "https://example.invalid/sets/hits.parquet",
            &file("hits.parquet", ""),
        );
        generated.corpus.category = Category::Generate;
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let error = corpus(&generated, &store, &mut |_| {}).unwrap_err();
        assert!(matches!(error, FetchError::NotFetchable { .. }));
    }

    #[test]
    fn a_corpus_already_in_the_store_is_not_requested_again() {
        // The source is unreachable on purpose. If this passes, nothing went near the network.
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        store.insert_bytes(b"").unwrap();

        let manifest = manifest(
            "https://nothing.invalid/hits.parquet",
            &file("hits.parquet", ""),
        );
        let mut seen = Vec::new();
        let fetched = corpus(&manifest, &store, &mut |progress| {
            seen.push(progress.done);
        })
        .unwrap();
        assert!(fetched[0].deduplicated);
        assert_eq!(fetched[0].digest.to_hex(), EMPTY_DIGEST);
        assert_eq!(seen, vec![1024]);
    }
}
