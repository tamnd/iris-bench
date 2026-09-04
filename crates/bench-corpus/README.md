# bench-corpus

Corpus manifests, fetching, generation and content addressed storage.

Every corpus is pinned by a BLAKE3 digest, including the ones that are

generated rather than downloaded. A digest mismatch on fetch is a hard

failure that prints both digests, because a corpus that quietly changed is

worse than one that is missing.

Part of [iris-bench](https://github.com/tamnd/iris-bench). Licensed under Apache-2.0.
