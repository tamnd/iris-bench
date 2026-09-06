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
//!
//! A mirrored corpus is downloaded by this same code. The category says who serves the bytes and
//! what permits them to, which is a licensing fact rather than a transport one, and it is why a
//! mirrored entry must carry its own `url`: `source` stays the place the data came from
//! originally, so resolving against it would send the fetch back to the host the mirror exists
//! because of.

use std::{io, time::Duration};

use crate::{
    digest::Digest,
    manifest::{Category, Entry, Manifest},
    progress::{Counting, Progress},
    store::{Store, StoreError},
};

/// How long to wait for the far end to start answering.
///
/// There is no limit on the transfer itself. A corpus here is tens of gigabytes and the honest
/// upper bound on how long that takes on an unknown link is not a number anybody can write down, so
/// the timeout that exists is the one that catches a host which is not answering at all.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

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

/// Why a corpus did not arrive.
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    /// The corpus is not one that can be downloaded.
    #[error(
        "{name} is a {category} corpus, so there is nothing to fetch. It is produced locally, by \
         `iris-bench corpus {name} --generator <path>`"
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
    // A mirrored corpus is downloaded the same way a fetched one is, over the same HTTP, checked
    // against the same digests. The category is about who is serving the bytes and what permits
    // them to, which is a licensing fact rather than a transport one. Only a generated corpus has
    // nothing to download.
    if manifest.corpus.category == Category::Generate {
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

    let mut counting = Counting::new(
        response.into_body().into_reader(),
        &entry.path,
        entry.bytes,
        watch,
    );
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
    fn a_mirrored_corpus_is_downloaded_like_any_other() {
        // The source is unreachable on purpose, and the store already holds the one file, so this
        // passing means the mirror category got as far as asking the store rather than being
        // refused for being the wrong kind of corpus.
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        store.insert_bytes(b"").unwrap();

        let mut mirrored = manifest(
            "https://nothing.invalid/silesia.zip",
            &file("dickens", "url = \"https://ours.invalid/silesia-dickens\""),
        );
        mirrored.corpus.category = Category::Mirror;
        let fetched = corpus(&mirrored, &store, &mut |_| {}).unwrap();
        assert!(fetched[0].deduplicated);
    }

    /// A server that answers one request with `body` and stops, on a port the operating system
    /// picks so that tests running at the same time do not collide.
    ///
    /// Worth the twenty lines. Everything else about a fetch can be tested without a network, and
    /// the one thing that cannot is what happens when a real download turns out to be the wrong
    /// bytes, which is the failure this whole crate is built around.
    fn serving(body: &'static [u8]) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read as _, Write as _};

        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let url = format!("http://{}/example.bin", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let Ok((mut socket, _)) = listener.accept() else {
                return;
            };
            let mut request = [0_u8; 1024];
            let _ = socket.read(&mut request);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = socket.write_all(head.as_bytes());
            let _ = socket.write_all(body);
        });
        (url, handle)
    }

    /// A manifest pinning one file at `digest` and `bytes`, served from `url`.
    fn pinning(url: &str, digest: &Digest, bytes: usize) -> Manifest {
        manifest(
            url,
            &format!(
                "[[files]]\npath = \"example.bin\"\nblake3 = \"{}\"\nbytes = {bytes}\n",
                digest.to_hex()
            ),
        )
    }

    #[test]
    fn a_download_that_is_what_was_pinned_lands_in_the_store() {
        let (url, server) = serving(b"what actually arrived");
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let promised = Digest::of_bytes(b"what actually arrived");

        let fetched = corpus(&pinning(&url, &promised, 21), &store, &mut |_| {}).unwrap();
        server.join().unwrap();

        assert_eq!(fetched[0].digest, promised);
        assert!(store.contains(&promised));
    }

    #[test]
    fn a_download_that_is_not_what_was_pinned_is_refused_and_nothing_is_kept() {
        // Same length as what the server sends, so the size check cannot fire first and this is
        // known to be the digest refusing it.
        let (url, server) = serving(b"what actually arrived");
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let promised = Digest::of_bytes(b"what was promised!!!!");

        let error = corpus(&pinning(&url, &promised, 21), &store, &mut |_| {}).unwrap_err();
        server.join().unwrap();

        assert!(matches!(error, FetchError::Digest { .. }));
        let message = format!("{error}");
        assert!(message.contains("not overridable"), "{message}");
        assert!(message.contains(&promised.to_hex()), "{message}");
        assert!(message.contains(&url), "{message}");

        // Neither under the name it was promised as, nor under the name it turned out to have.
        assert!(!store.contains(&promised));
        assert!(!store.contains(&Digest::of_bytes(b"what actually arrived")));
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
