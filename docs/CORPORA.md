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

An entry may also carry `url`, and almost no fetched corpus does. Without it the file's URL is its path resolved against the corpus `source` in the ordinary way, meaning everything after the last slash of `source` is replaced by the path. For a single file corpus whose source is the file itself that resolves back to the source unchanged, which is why the common case needs nothing written down. For a corpus of many files under one prefix, `source` names the directory with a trailing slash and each path is appended. `url` exists for the case where one corpus is assembled from files that are not under a common prefix, which happens often enough in published datasets that a format with no answer for it would mean forking a corpus rather than describing it.

A mirrored corpus must carry `url` on every entry, and that is checked. Its `source` stays the place the data came from originally, because that is the provenance and a mirror that has lost track of where its data came from is worse than a dead link, so resolving a mirrored file against `source` would send the fetch back to the host the mirror exists because of.

`part_of` names the corpus this one is a selection from, and only a corpus that really is one may carry it. Public BI is why it exists. The benchmark is published as 206 tables, much of the encoding literature measures a subset of 36 of them, and a number labelled Public BI is ambiguous about which of those two it is. So both are corpora here, `public-bi` and `public-bi-36`, and the second says which set it is drawn from.

That claim is checked rather than believed. Every entry in a part has to appear in the whole at the same path with the same digest and the same size, and the part has to be smaller. A subset that has quietly drifted from the set it names is worse than no subset at all, because every number labelled with it is then about something nobody can reconstruct. The check needs both manifests in hand, so it runs in `ci/discipline.py` over the tree and in `Manifest::is_part_of` at run time, while the parts of the claim that can be settled from one manifest alone, that the name is not empty and not the corpus itself, are checked where every other field is.

`[generator]` says what produces a generated corpus, and only a generated corpus may have one. A manifest carrying both a generator and a download location is making two claims about one set of bytes, and the second of those is the one that goes unread. It has a `program` to run, `arguments` to pass it, a `version` that must appear in what the program says about itself, `version_arguments` that make it say so, and an `environment` table for anything the program needs to be told through the environment rather than on the command line. The working directory is the output directory, and two placeholders are substituted into environment values: `{output}` is where the files should land and `{program_directory}` is where the program itself was found.

It also has `platforms`, which is the list of operating systems the pinned bytes have been produced on, spelled the way `std::env::consts::OS` spells them, so `linux`, `macos` and `windows`. It is required and may not be empty, and generating on a platform that is not in the list is refused rather than attempted. Whether a generator's output depends on the platform is a question about that particular generator rather than about generators in general, so the answer lives in the manifest as data instead of in the code as a rule. The Generating section below says which platforms TPC-H names and what settled it.

The version is part of the pin and is checked before generation starts. The bytes a generator writes are a property of the generator, so a manifest pinned against one build of `dbgen` says nothing about another, and a digest mismatch on twenty gigabytes of output has nothing in it to suggest that the cause is a release two versions along. One process start buys that sentence.

The generator itself is never in this repository. TPC's tools are downloaded under TPC's own end user agreement by whoever runs the generation, which is why `--generator` exists on `iris-bench corpus` and why the error when the program is absent says where to get one rather than what a missing file is.

`[assertions]` is optional and is checked after the corpus loads rather than after it downloads. ClickBench is 99,997,497 rows across 105 columns or the manifest is wrong, and an assertion catches that far more cheaply than a number that looks slightly off six weeks later. Both fields are optional because not every corpus is tabular, and a corpus that asserts neither gets told so on every fetch rather than passing quietly.

Rows are summed across the files of a corpus and columns have to agree between them, because a corpus split across files is one table and files with different schemas are not one table however they are named. Both counts come out of the Parquet footer, which is a seek and a few kilobytes rather than a pass over the data, and that is what makes it affordable to check before every measurement instead of once when the corpus was added.

Only files whose declared path ends in `.parquet` are read for a shape, and that is decided from the manifest rather than from the bytes because the manifest is this repository's own statement about what a file is. Silesia and enwik8 are compressed text with no rows or columns at all, and for them the shape check does not apply rather than fails. A file named `.parquet` that turns out not to be Parquet is still a hard error, and a manifest that asserts a shape for a corpus where nothing can carry one is refused, because an assertion nothing checks against is worse than no assertion: on the page it reads like something is being verified.

Unknown fields are an error rather than being ignored. The field most worth typing wrongly is `licence_note`, and a typo that silently does nothing would defeat the one check on this page that has legal weight.

## The corpus identity

A corpus has one digest of its own, printed by `iris-bench corpus` before anything is fetched and computed by `Manifest::identity`. It is taken over the corpus name and then every entry's path, digest and size, sorted by path.

It exists so that a result row can say which corpus it used in one value. `public-bi` and `public-bi-36` are both honestly called Public BI, and even within one of those, a corpus can gain or lose a table between one measurement and the next. A name does not distinguish those cases and the identity does.

Sorted by path, so reordering entries in the file does not move it. Taken over the entries rather than over the manifest text, so a rewritten comment does not move it either. That second one is deliberate: a comment is not a corpus, and an identity that changed when somebody fixed a typo would teach everybody to ignore the field, which costs more than the precision it buys.

## Digests

Lower case hex, always, and an upper case spelling is refused rather than normalised. A manifest is read by people as well as by code, and two spellings of one digest is how a mismatch turns into an argument about whether it is a mismatch.

BLAKE3 rather than SHA-256, because hashing a hundred gigabyte corpus has to be cheap enough that nobody is ever tempted to skip it. A check that only runs when somebody remembers to run it is not a pin. On the i9-13900K under Linux, the 13.8 GiB ClickBench file hashes in 2.4 seconds off a warm page cache, which is about 5.7 GiB per second, so on that machine the check is bounded by the disk rather than by the hash.

A digest mismatch is a hard failure with no override, and the message carries both digests. A mismatch reported as a boolean is a mismatch somebody has to reproduce before they can begin working out what happened. The section below is about the no override part, which is the half that is easy to mean and hard to keep.

The size is checked as well as the digest, even though the digest would catch anything the size would. They are reported separately because a wrong length is a truncated download and a full length mismatch is a different file, and those two send somebody to look in completely different places.

## No override

There is no flag, no environment variable and no prompt. A corpus whose bytes changed is a different corpus, and continuing anyway is how a silently corrupted download becomes a published number six weeks later. The command fails, nothing is kept, and the exit status says so.

Nothing is kept is a separate claim from the command fails, and it is the one that takes work. The store writes through a temporary file under the name the manifest promised, because the digest is not final until the last byte has arrived, so a mismatch means removing an object that is already in place. What is left afterwards is neither the object under the name it was promised as nor the object under the name it turned out to have. The second of those is a real temptation: an object addressed by its own digest is exactly what this store holds, so keeping the wrong bytes would be defensible and would also mean a corpus that changed at its source quietly accumulates in every store it reaches.

There is no override today, and the point of this section is that there cannot be one tomorrow either without somebody deliberately removing the check that says so. `check_digest_is_not_overridable` in `ci/discipline.py` reads the four files a corpus's bytes pass through, which are `fetch.rs`, `generate.rs`, `store.rs` and the CLI's `corpus.rs`, and fails the build on three things.

An override name. `force`, `no_verify`, `skip_verify`, `allow_mismatch`, `insecure` and the rest of that family, matched as whole words so that `eq_ignore_ascii_case` is not an override and `enforce` is not `force`. This is a list of spellings rather than a rule about meaning, which is the right shape for it: the thing being prevented is somebody adding one in a hurry, and a hurry does not invent new vocabulary.

A read of the environment. `env::var` and `option_env!` are refused in those four files. An environment variable is what an override reaches for first, because it leaves no trace in the command somebody pastes into an issue, so a run that behaved differently is indistinguishable afterwards from one that did not. `env!("CARGO_PKG_VERSION")` and `std::env::consts::OS` are not reads of the environment a run happens in and are not affected.

An insert that names no digest. Every byte of a corpus enters the store through `insert_stream` or `insert_verified`, both of which take the digest the manifest pinned. `insert_bytes` and `insert_file` name whatever they are given, which is the right behaviour for a content addressed store and the wrong one for a corpus, and reaching for one of them in the fetch or generate path is how the check would get skipped without anybody having to write the word skip.

All three rules stop at the first `#[cfg(test)]`, because a test is allowed to say words a run may not act on, and the tests that prove a mismatch is fatal say most of them.

Those tests are the other half. `bench-corpus` stands up a one request HTTP server on a port the operating system picks, serves bytes that are not what the manifest pinned at exactly the pinned length so the size check cannot fire first, and checks that the fetch fails, that the message names both digests and the URL, and that neither digest is in the store afterwards. The same server serving the right bytes is a second test, so the first one is known to be failing on the digest rather than on the scaffolding. On the generated side, `iris-bench corpus` is run end to end against a generator that writes the wrong bytes, and it fails with no object left behind. That last one is at the level a person actually uses, which is where a warning above a corpus that then gets measured would have shown up.

## Fetching

`iris-bench corpus <name>` reads the manifest, fetches what it names, and checks what arrived, in that order. A corpus that describes itself badly never reaches the network, bytes that are not what was promised never reach the store, and a corpus that is short never reaches a measurement.

The digest is computed in the same pass that writes the file rather than by reading it back afterwards. That is not only about speed on a fifteen gigabyte download, although it is that too. It means a file that does not match is deleted rather than left sitting under a name that asserts its own content, so the store's one invariant holds even when a fetch fails halfway.

A file the store already has is never requested. That is not a cache, it is what content addressing means: the manifest already said which bytes it wants, and the store either has those bytes or it does not. Fetching ClickBench a second time on a machine that already has it takes 56 milliseconds including reading the footer and checking the assertions.

Public BI is the largest corpus here by file count rather than by size. The full set is 206 tables across 46 workbooks and 43,334,957,146 bytes of bzip2 compressed CSV, about 386 GB once decompressed. The 36 table subset is 10,547,903,083 bytes. On the 4 vCPU AMD EPYC guest the subset fetched and verified in 19 minutes 16 seconds and the second run took 0.53 seconds, and peak resident memory across the 9.8 GiB fetch was 7.8 MB, because the digest is computed on the bytes as they stream past rather than on a file read back afterwards.

The full set then fetched on the same machine in 45 minutes 6 seconds, at a peak of 8.7 MB, and reported that 62 of its 206 files were already in the store. That number is the whole argument for content addressing in one line. 36 of the 62 were there because the subset had already been fetched, and the other 26 were never downloaded at all, because they are duplicates of tables that arrived earlier in the same run.

206 declared files come to 180 distinct objects. Twenty six of the MLB tables are byte for byte a duplicate of another MLB table, because Tableau extracted that workbook several ways and some of the extracts came out identical. Nothing was written to handle that. The store is addressed by content, so the duplicates collapse on the way in and the corpus still declares all 206 because the benchmark does.

The first fetch of ClickBench on the i9-13900K under Linux took about twenty minutes for 13.8 GiB, arrived at the digest pinned in the manifest, and reported 99,997,497 rows across 105 columns. Those numbers were pinned before the fetch ran, from a separate download hashed with `b3sum` and from the ClickBench documentation, so this was the manifest being checked rather than written.

## Mirroring

A mirrored corpus is downloaded by the same code that fetches any other one, over the same HTTP and against the same digests. The category is about who is serving the bytes and what permits them to, which is a licensing question rather than a transport one, and `docs/LICENSING.md` is where it is answered.

The mirror itself is a release on this repository, tagged `corpora-mirror-1`, with one asset per file. That is a deliberate choice over the two obvious alternatives. Committing the files into the tree would put a few hundred megabytes into every clone forever, including every CI job, to serve a corpus that most runs never ask for. Hosting them on a machine in the fleet would tie a public repository to a private host, and to that host still existing. Release assets are served by the same infrastructure as the repository itself, so a mirror that has stopped working and a repository that has stopped existing are the same event.

Nothing about the mirror is trusted. `iris-bench corpus silesia` checks the digest and the byte count of every file in the same pass that writes it, so an asset that was replaced fails the fetch rather than producing a number. That is the same guarantee as for a corpus downloaded from its author, which is the point: mirroring changes who serves the bytes, and changes nothing about what is believed.

Silesia is twelve files and 211,938,580 bytes, and enwik8 is one file and exactly 100,000,000 bytes, which is a definition rather than a coincidence, since the Large Text Compression Benchmark and the Hutter Prize are stated over a prefix of a fixed length. On the 4 vCPU AMD EPYC guest, Silesia fetched and verified in 33.1 seconds and enwik8 in 20.4 seconds, both at a peak of about 8 MB of resident memory, and both came back in under a hundredth of a second on the second run with every file already in the store.

## Generating

`iris-bench corpus <name> --generator <path>` is the same command with the bytes produced locally instead of downloaded. The order is the same and so is what it buys: the manifest is validated, the store is asked what it already has, the generator's version is checked, the scratch directory is emptied, the generator runs, and only then is each declared file digested and inserted.

The dedup check comes before the generator starts rather than before each file, which is the one place the generated path differs from the fetched one and the difference is not cosmetic. Fetching is per file, so skipping a file that is already present skips the download of that file. A generator writes its whole output in one run, so the question worth asking is whether the entire corpus is already in the store, and asking it early is what turns a second `tpch-sf20` on a machine that already has it from three minutes into nothing.

The scratch directory is emptied rather than written into. A generator that fails halfway through leaves a short file behind, and a short file with the right name is exactly the input that makes the next run's digest mismatch look like a generator bug.

The version probe runs the generator with `version_arguments`, reads stdout and stderr together, and ignores the exit status. A program printing its usage commonly exits non zero, and `dbgen -h` does, so treating that as a failure would mean no version could ever be checked. What matters is whether the pinned version string appears in what came back.

Two placeholders are substituted into the environment values. `{output}` is the scratch directory, which is where the files are expected to land. `{program_directory}` is the directory the generator was found in, which `dbgen` needs because it reads its column distributions from `dists.dss` sitting beside the binary rather than from anywhere a package manager would put it. Both of those are facts about `dbgen` and they live in its manifest, so the code that runs a generator knows nothing about any particular one.

A generated corpus carries no `[assertions]` block, and that is a rule about what an assertion is for rather than an omission. ClickBench asserts its shape because its digest is a claim about a file on somebody else's server, so a second check that arrives at the same number by a different route is worth having. For a corpus generated and pinned file by file, 6,001,215 lineitem rows is what those bytes are, not something they could fail to be, and a row count asserted next to a digest that already implies it reads on the page like two things are being verified.

TPC-H is generated with `dbgen` 2.17.3, which is not in this repository. TPC's own tools are downloaded under TPC's end user agreement by whoever runs the generation. On macOS the build is `make CC=cc DATABASE=POSTGRESQL MACHINE=MACOS WORKLOAD=TPCH`, and on Linux with gcc 15 it needs `CC="cc -std=gnu17"` because gnu23 is now the default and the K&R prototypes in that source do not survive it.

Scale factor 1 is 1,092,031,885 bytes across eight files and 8,661,245 rows. It generates in 12.75 seconds on the i9-13900K under Linux and in 2 minutes 20 on the Apple silicon laptop. It exists so a change can be tried in under a minute.

Scale factor 20 is 22,459,651,466 bytes and 173,194,638 rows, and generates in 196.33 seconds on the i9-13900K under Linux. 20 is deliberately not a compliant TPC-H scale factor. It is the size that stops fitting comfortably in cache on the workstation class, and a benchmark that fits in cache measures the cache. Every published row carrying one of these numbers is marked as a deviation by `bench_report` rather than in a footnote somebody writes.

`nation.tbl` and `region.tbl` are byte for byte identical between the two scale factors, because those two tables do not scale. The store holds them once across both corpora, which is the content addressing doing what it was built for rather than a case anybody handled.

All eight scale factor 1 digests are identical on macOS on Apple silicon, on Linux on x86-64 and on Linux on arm64, so what is pinned is stable across two operating systems and two architectures rather than across two runs on one machine. The arm64 Linux leg was produced in a native aarch64 container from the same `dbgen` 2.17.3 source, took 22 minutes 11 seconds, and came out at the corpus identity `efab179a79dd64800aa15bcb0ec01707215930552b98fce3991027c3f62d3612`, which is what the other two produce.

Windows is the fourth platform and it is the one where these digests cannot be produced, for a reason in `dbgen` rather than in this repository. Its `print.c` opens each table with `fopen(upath, "w")`, which is text mode, so the Windows C runtime expands every newline into a carriage return and a newline on the way out. Every one of the eight files ends its lines with a bare newline, so on Windows every file would be longer than its pinned size and not one digest could match. That is not fixable from here without shipping a patched `dbgen`, which would no longer be the 2.17.3 the manifest pins and would make the pin a claim about our patch rather than about TPC's tool.

There is a second reason, independent of the first, which is that the published source does not build there with the toolchain we have. mingw-w64 predefines `WIN32`, so `config.h` takes its Windows branch, and that branch spells its constants with MSVC integer suffixes such as `#define RNG_A 6364136223846793005uI64`. gcc rejects them with `invalid suffix "uI64" on integer constant`. Setting `MACHINE=LINUX` does not help, because the branch is chosen by the predefined macro and not by the makefile. The Windows host reachable from the fleet has no `cl.exe` installed, so MSVC has not been tried and the first reason stands on its own anyway.

So one platform is characterised rather than measured, and it is blocked in code rather than in prose. `[generator] platforms` lists the operating systems on which a corpus's pinned bytes have actually been produced, and generating anywhere else is refused before the generator is started. TPC-H at both scale factors says `["linux", "macos"]`. A Windows machine asking for `tpch-sf1` is told which platforms it is pinned from, what it is, and that running the generator anyway would fail the digest check rather than produce a corpus. That last sentence is the point of the whole mechanism: a mismatch on a gigabyte of output reads like a corrupted download, and it takes an afternoon to discover that the cause was the C runtime rewriting newlines.

The field is required and non empty for every generator, in the parser and in `ci/discipline.py`, so a corpus that has never said where its bytes reproduce cannot be added. Refusing to generate is not the same as refusing to use. A corpus already in the store is served on any platform, because bytes do not care what wrote them, and only the making of them is tied to a platform.

Scale factor 20 was generated twice on unrelated machines, on the i9-13900K under Ubuntu 25.10 with gcc 15.2 and on a 4 vCPU AMD EPYC guest under Ubuntu 24.04, and all eight digests and all eight row counts came out the same. Different CPU vendor, different distribution, different compiler. The EPYC guest took about eighteen minutes against 196.33 seconds on the workstation, which is a fact about the guest rather than about `dbgen`.

Scale factor 20 has not been generated on arm64 Linux, because no machine in the fleet has 22 GiB of free disk on that architecture. That is a gap in the evidence and it is written here rather than left to be inferred from what the table does not say.

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

`check_digest_is_not_overridable` is the exception to that pattern, because it has no Rust counterpart and cannot have one. It is a check on the source rather than on a manifest, so the thing it would have to run against is itself.
