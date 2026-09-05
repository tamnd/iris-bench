# Corpora and the manifest format

A corpus that is not pinned by digest is not a corpus, it is whatever happened to download that day. This page is the format that does the pinning and the store the bytes end up in. What each corpus is licensed under and how this repository is allowed to handle it is in `LICENSING.md`, which this format encodes rather than replaces.

## Where a manifest lives

One directory per corpus under `corpora/`, and one `manifest.toml` in it. The directory name is the corpus name, and a manifest that disagrees with its own directory is refused rather than being taken as authoritative over it. Two names for one corpus is how a result ends up citing something other than what it read.

## The format

```toml
[corpus]
name = "example"
description = "One sentence, for a reader who has not met this corpus before"
source = "https://example.invalid/example.parquet"
licence = "Apache-2.0"
category = "fetch"

[[files]]
path = "example.parquet"
blake3 = "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
bytes = 14779976446

[assertions]
rows = 99997497
columns = 105
```

`name` is the identifier every other document cites and is also the directory name.

`description` is one sentence. It exists so that somebody reading a result table can find out what they are looking at without leaving the repository.

`source` is where the bytes come from: a URL for anything downloaded, or the generator and its arguments for anything produced locally.

`licence` is an SPDX identifier where one exists and prose where one does not.

`category` is `generate`, `fetch` or `mirror`, and the three are defined in `LICENSING.md`. The default is `fetch`. A `mirror` corpus must also carry `licence_note`, which says in plain terms what permits this repository to redistribute it. That field is checked rather than encouraged, because the moment it gets skipped is the moment somebody is in a hurry.

`[[files]]` is one entry per file, each with a path relative to the corpus directory, a BLAKE3 digest in lower case hex, and a size in bytes. A manifest with no files is refused, because it pins nothing and the pin is the entire point. So is a path that is absolute or that contains a parent segment, since a manifest is as often something fetched from elsewhere as something written here. So is a file declared as zero bytes, and so is the same path listed twice.

`[assertions]` is optional and is checked after the corpus loads rather than after it downloads. ClickBench is 99,997,497 rows across 105 columns or the manifest is wrong, and an assertion catches that far more cheaply than a number that looks slightly off six weeks later. Both fields are optional because not every corpus is tabular.

Unknown fields are an error rather than being ignored. The field most worth typing wrongly is `licence_note`, and a typo that silently does nothing would defeat the one check on this page that has legal weight.

## Digests

Lower case hex, always, and an upper case spelling is refused rather than normalised. A manifest is read by people as well as by code, and two spellings of one digest is how a mismatch turns into an argument about whether it is a mismatch.

BLAKE3 rather than SHA-256, because hashing a hundred gigabyte corpus has to be cheap enough that nobody is ever tempted to skip it. A check that only runs when somebody remembers to run it is not a pin.

A digest mismatch on fetch is a hard failure with no override, and the message carries both digests. A mismatch reported as a boolean is a mismatch somebody has to reproduce before they can begin working out what happened.

## The store

Verified bytes go into a content addressed store, and an object is named by its own digest:

```text
<root>/objects/af/1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262
```

Two runs referring to the same corpus are then provably referring to the same bytes rather than to the same file name. A file name is a claim about content and a digest is the content, and the distance between those two is where an unreproducible result comes from.

The first byte of the digest becomes a directory. That is not about lookup speed, since every lookup here is a direct path from a digest that is already known. It is about any one directory staying listable by a person trying to work out what is on a machine.

Deduplication is not a feature, it is what the naming does. Two corpora sharing a file store it once, and re-fetching a corpus that is already present writes nothing at all.

Writes go through a temporary file that is renamed into place once it is complete. A process that dies halfway through leaves a stray temporary file rather than a truncated object sitting at a path that asserts its own content, and that second thing is the one failure the store exists to make impossible. A file is copied in rather than moved, because the source is usually either a download somebody wants to keep or a file inside an extracted archive that other entries in the same manifest also point into.

## What is checked, and where

Twice, deliberately, and the duplication is not an accident.

`ci/discipline.py` validates every manifest committed to this repository and fails the build. It runs on a tree that may not compile, which is the case where a bad manifest is most likely to slip past, so it cannot be the Rust code.

`bench_corpus::Manifest` validates the same rules at run time. It runs on manifests that are not in this repository at all, which is where the format is heading once corpora are contributed rather than curated.

The two lists are kept in step by hand. If they drift, the Python one is the one that gates the build and the Rust one is the one that gates a run, and either catching something the other misses is a bug in the one that missed it.
