# bench-corpus

Corpus manifests, fetching, generating, verifying, and the content addressed store.

Every corpus is pinned by a BLAKE3 digest, including the ones that are generated rather than downloaded. The digest is checked in the same pass that writes the file, so bytes that are not what the manifest promised never land in the store, and a mismatch is a hard failure that prints both digests with no override anywhere. A corpus that quietly changed is worse than one that is missing, because the missing one stops a run and the changed one does not.

The asserted row and column counts are checked after the digest, which is what catches a download that stopped early and still parses as a valid file.

A generated corpus is pinned to the platforms it has actually been produced on, and generating anywhere else is refused with a reason rather than attempted. The generator's own version is checked before it produces a byte, because a different generator writes different bytes and a digest mismatch on its own does not say which of those two things went wrong.

`docs/CORPORA.md` is the manifest format and what each pinned corpus is.

Part of [iris-bench](https://github.com/tamnd/iris-bench). Licensed under Apache-2.0.
