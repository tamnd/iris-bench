#!/usr/bin/env python3
"""Checks that are about discipline rather than about compilation.

The method in docs/METHODOLOGY.md is only real if it is mechanised. This script
is the mechanisation. It runs in CI on every change and it fails the build.

What it checks:

  1. Every driver in drivers/ has a CONFIG.md with all its required sections.
  2. Every driver's configuration review date is present and parseable, and is
     not more than 180 days old.
  3. Every corpus manifest matches the format in docs/CORPORA.md: it names
     itself after its own directory, declares a description, a source, a licence
     and one of the three categories, pins at least one file by lower case hex
     BLAKE3 digest and size, uses paths that stay inside the corpus directory,
     and carries a licence note if it is mirrored.
  4. No driver crate depends on anything in the tree except bench-driver.
  5. No hostname, address or account identifier from the benchmark fleet has
     leaked into a committed file.
"""

from __future__ import annotations

import datetime as dt
import pathlib
import re
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
REVIEW_MAX_AGE_DAYS = 180

CONFIG_SECTIONS = [
    "Source",
    "Settings",
    "Deviations",
    "Rejected alternatives",
    "Last reviewed",
]

# The three handling categories from docs/LICENSING.md. A corpus is generated
# locally, fetched at run time, or redistributed here, and the third one is the
# only one that needs a licence note because it is the only one where this
# repository is the party doing the redistributing.
CORPUS_CATEGORIES = {"generate", "fetch", "mirror"}

# Lower case hex, always. An upper case spelling of the same digest is refused
# rather than normalised, because a manifest is read by people as well as by
# code and two spellings of one value is how a mismatch gets argued about.
DIGEST = re.compile(r"[0-9a-f]{64}")

# Every field this format has. A field that is not here is an error rather than
# something ignored, because the field most worth typing wrongly is
# licence_note, and a typo that silently does nothing would defeat the one check
# in this file with legal weight.
MANIFEST_FIELDS = {
    "corpus": {
        "name",
        "description",
        "source",
        "licence",
        "category",
        "licence_note",
        "part_of",
    },
    "files": {"path", "blake3", "bytes", "url"},
    "generator": {
        "program",
        "arguments",
        "version",
        "version_arguments",
        "platforms",
        "environment",
    },
    "assertions": {"rows", "columns"},
}

# A driver may depend on the trait crate and on nothing else in this workspace.
# Otherwise a driver can reach into the runner and special case itself, which is
# the single failure mode that would make every comparison here worthless.
DRIVER_ALLOWED_INTERNAL_DEPS = {"bench-driver"}

# A driver does not hold the clock. bench_driver::Session times the three phases
# from the outside, and a driver that reached for a clock of its own would be
# reporting a number nobody else checked, which is how load time ends up inside
# preparation and preparation ends up in nobody's total.
DRIVER_CLOCK = re.compile(r"\bInstant\b|\bSystemTime\b|\bclock_gettime\b|\bQueryPerformanceCounter\b")

failures: list[str] = []


def fail(message: str) -> None:
    failures.append(message)


def check_drivers() -> None:
    drivers = sorted(p for p in (ROOT / "drivers").iterdir() if p.is_dir())
    if not drivers:
        fail("drivers/ is empty, which should not happen once B2 has landed")
    for driver in drivers:
        config = driver / "CONFIG.md"
        if not config.exists():
            fail(f"{driver.name}: no CONFIG.md")
            continue
        text = config.read_text(encoding="utf-8")
        for section in CONFIG_SECTIONS:
            if not re.search(rf"^##\s+{re.escape(section)}\s*$", text, re.MULTILINE):
                fail(f"{driver.name}: CONFIG.md has no '{section}' section")
        match = re.search(r"^##\s+Last reviewed\s*$\n+(\d{4}-\d{2}-\d{2})", text, re.MULTILINE)
        if not match:
            fail(f"{driver.name}: CONFIG.md has no review date in YYYY-MM-DD form")
        else:
            reviewed = dt.date.fromisoformat(match.group(1))
            age = (dt.date.today() - reviewed).days
            if age > REVIEW_MAX_AGE_DAYS:
                fail(f"{driver.name}: configuration last reviewed {age} days ago")
        check_driver_deps(driver)
        check_driver_source(driver)


def check_driver_source(driver: pathlib.Path) -> None:
    """A driver implements the trait, and does not time itself.

    The first half is what makes "every driver reports all three phases" true
    rather than hoped for: a crate under drivers/ that implements nothing is a
    system in the matrix that never gets measured. The second half is why the
    phases mean anything, since a driver holding its own clock can report a
    preparation that took no time and a load that happened somewhere else.
    """
    source = driver / "src" / "lib.rs"
    if not source.exists():
        fail(f"{driver.name}: no src/lib.rs")
        return
    text = source.read_text(encoding="utf-8")
    if not re.search(r"^impl Driver for ", text, re.MULTILINE):
        fail(f"{driver.name}: does not implement Driver, so nothing about it can be measured")

    lines = text.splitlines()
    end = next(
        (number for number, line in enumerate(lines) if line.strip() == "#[cfg(test)]"),
        len(lines),
    )
    for number, line in enumerate(lines[:end], 1):
        if line.lstrip().startswith("//"):
            continue
        hit = DRIVER_CLOCK.search(line)
        if hit:
            fail(
                f"{driver.name}:src/lib.rs:{number}: {hit.group()} is a clock, and the phases are"
                f" timed by the session rather than by the driver"
            )


def check_driver_deps(driver: pathlib.Path) -> None:
    manifest = tomllib.loads((driver / "Cargo.toml").read_text(encoding="utf-8"))
    internal = {
        name
        for name in manifest.get("dependencies", {})
        if name.startswith(("bench-", "driver-"))
    }
    stray = internal - DRIVER_ALLOWED_INTERNAL_DEPS
    if stray:
        fail(f"{driver.name}: depends on {sorted(stray)}, which drivers may not do")


def check_corpora() -> None:
    """Validates every manifest in the tree against docs/CORPORA.md.

    These rules are the same ones bench_corpus::Manifest applies, duplicated on
    purpose. This runs on a tree that may not compile, which is exactly the case
    where a bad manifest is most likely to get through, so it cannot be the Rust
    code that does it.
    """
    corpora = ROOT / "corpora"
    if not corpora.exists():
        return
    parsed: dict[str, dict] = {}
    for directory in sorted(p for p in corpora.iterdir() if p.is_dir()):
        manifest_path = directory / "manifest.toml"
        if not manifest_path.exists():
            fail(f"corpus {directory.name}: no manifest.toml")
            continue
        check_manifest(manifest_path)
        try:
            parsed[directory.name] = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
        except tomllib.TOMLDecodeError:
            pass
    check_parts(parsed)


def check_parts(manifests: dict[str, dict]) -> None:
    """Checks that a corpus claiming to be part of another really is one.

    Public BI is the case. The 36 table subset much of the encoding literature
    measures is only meaningful if it is provably 36 of the 206, and a subset
    that has quietly drifted from the set it names is worse than no subset at
    all, because every number labelled with it is then a number about something
    nobody can reconstruct.
    """
    for name, manifest in sorted(manifests.items()):
        whole_name = str(manifest.get("corpus", {}).get("part_of", "")).strip()
        if not whole_name:
            continue
        if whole_name == name:
            fail(f"corpus {name}: says it is part of itself")
            continue
        whole = manifests.get(whole_name)
        if whole is None:
            fail(f"corpus {name}: says it is part of '{whole_name}', which is not in corpora/")
            continue
        held = {
            entry.get("path"): (entry.get("blake3"), entry.get("bytes"))
            for entry in whole.get("files", [])
        }
        for entry in manifest.get("files", []):
            path = entry.get("path")
            if path not in held:
                fail(f"corpus {name}: pins '{path}', which is not in '{whole_name}'")
            elif held[path] != (entry.get("blake3"), entry.get("bytes")):
                fail(f"corpus {name}: pins '{path}' differently from '{whole_name}'")
        if len(manifest.get("files", [])) >= len(whole.get("files", [])):
            fail(f"corpus {name}: is not smaller than '{whole_name}', so it is not a part of it")


def check_fields(name: str, table: str, values: dict, allowed: set[str]) -> None:
    """Complains about any field in a manifest table that this format does not have."""
    if not isinstance(values, dict):
        fail(f"corpus {name}: '{table}' is not a table")
        return
    for field in sorted(set(values) - allowed):
        fail(f"corpus {name}: '{field}' in [{table}] is not part of this format, see docs/CORPORA.md")


def check_manifest(manifest_path: pathlib.Path) -> None:
    directory = manifest_path.parent.name
    try:
        manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
    except tomllib.TOMLDecodeError as error:
        fail(f"corpus {directory}: manifest is not valid TOML: {error}")
        return

    corpus = manifest.get("corpus", {})
    name = corpus.get("name") or directory

    for table in manifest:
        if table not in MANIFEST_FIELDS:
            fail(f"corpus {name}: '{table}' is not part of this format, see docs/CORPORA.md")
    check_fields(name, "corpus", corpus, MANIFEST_FIELDS["corpus"])
    check_fields(name, "assertions", manifest.get("assertions", {}), MANIFEST_FIELDS["assertions"])
    check_fields(name, "generator", manifest.get("generator", {}), MANIFEST_FIELDS["generator"])
    for entry in manifest.get("files", []):
        check_fields(name, "files", entry, MANIFEST_FIELDS["files"])

    for field in ("name", "description", "source", "licence", "category"):
        if not str(corpus.get(field, "")).strip():
            fail(f"corpus {name}: manifest has no '{field}'")

    # The directory is the corpus name. Two names for one corpus is how a result
    # ends up citing something other than what it read.
    if corpus.get("name") and corpus["name"] != directory:
        fail(f"corpus {name}: named '{corpus['name']}' but lives in '{directory}'")

    if corpus.get("category") not in CORPUS_CATEGORIES:
        fail(
            f"corpus {name}: category is {corpus.get('category')!r} and the three"
            f" in docs/LICENSING.md are {sorted(CORPUS_CATEGORIES)}"
        )

    # Only a mirrored corpus is one this repository redistributes, so it is the
    # only one where somebody has to have read the licence and written down what
    # it permits.
    if corpus.get("category") == "mirror" and not str(corpus.get("licence_note", "")).strip():
        fail(f"corpus {name}: mirrored without a licence note saying what permits it")

    check_generator(name, corpus, manifest.get("generator"))

    files = manifest.get("files", [])
    if not files:
        fail(f"corpus {name}: manifest declares no files, so nothing is pinned")

    seen: set[str] = set()
    for entry in files:
        path = str(entry.get("path", "")).strip()
        if not path:
            fail(f"corpus {name}: a file entry has no path")
            continue
        if path in seen:
            fail(f"corpus {name}: {path} is listed twice")
        seen.add(path)
        if pathlib.PurePosixPath(path).is_absolute() or ".." in pathlib.PurePosixPath(path).parts:
            fail(f"corpus {name}: {path} is not a path inside the corpus directory")
        digest = str(entry.get("blake3", ""))
        if not DIGEST.fullmatch(digest):
            fail(f"corpus {name}: {path} has no lower case hex BLAKE3 digest")
        if not isinstance(entry.get("bytes"), int) or entry.get("bytes", 0) <= 0:
            fail(f"corpus {name}: {path} declares no size, so a truncated file passes the length check")
        # A fetched corpus has to be fetchable from what the manifest says, and the only two places
        # that can come from are the entry's own url or the corpus source resolved against it.
        if corpus.get("category") == "fetch" and not str(entry.get("url", "")).strip():
            if not str(corpus.get("source", "")).startswith(("http://", "https://")):
                fail(
                    f"corpus {name}: {path} has no url and the source is not one either,"
                    f" so there is nowhere to fetch it from"
                )
        # A mirrored corpus is served by this repository and its source stays the place the data
        # came from originally, so the address the bytes come from has to be on the entry.
        # Resolving it against the source would send the fetch back to the host the mirror exists
        # because of.
        if corpus.get("category") == "mirror" and not str(entry.get("url", "")).strip():
            fail(
                f"corpus {name}: is mirrored and {path} has no url, so nothing says where"
                f" this repository serves it from"
            )


def check_generator(name: str, corpus: dict, generator: dict | None) -> None:
    """A generated corpus says what produces it, and no other kind may.

    A manifest carrying both a generator and a download location is making two
    claims about one set of bytes, and the second of those is the one that would
    go unread.
    """
    category = corpus.get("category")
    if category == "generate" and generator is None:
        fail(f"corpus {name}: generated without a [generator], so nothing says how")
        return
    if generator is not None and category != "generate":
        fail(
            f"corpus {name}: has a [generator] and is a {category!r} corpus, which are two"
            f" different answers to where the bytes come from"
        )
        return
    if generator is None:
        return

    # The source of a generated corpus names the generator, so a URL there is a
    # statement that these bytes are downloaded, which they are not.
    if str(corpus.get("source", "")).startswith(("http://", "https://")):
        fail(
            f"corpus {name}: is generated and its source is a URL, so the manifest says both"
            f" that the bytes are produced here and that they are downloaded"
        )

    for field in ("program", "version"):
        if not str(generator.get(field, "")).strip():
            fail(f"corpus {name}: the generator has no '{field}'")

    # The version is what turns a digest mismatch on gigabytes of output into a
    # sentence about which dbgen is installed, so an unprobeable version is worse
    # than no version at all: it reads on the page like something is checked.
    probe = generator.get("version_arguments", ["-h"])
    if not isinstance(probe, list) or not probe:
        fail(f"corpus {name}: the generator has no version_arguments, so its version cannot be checked")

    if not isinstance(generator.get("arguments", []), list):
        fail(f"corpus {name}: the generator's arguments are not a list")

    # The digests of a generated corpus are of output somebody watched a generator
    # write on a particular machine, and a generator that writes different bytes
    # elsewhere is not hypothetical. Naming the platforms turns that into a refusal
    # with a reason instead of a digest mismatch with no explanation in it.
    platforms = generator.get("platforms", [])
    if not isinstance(platforms, list) or not platforms:
        fail(
            f"corpus {name}: the generator has no platforms, so nothing says where its"
            f" output was shown to reproduce"
        )
    else:
        for platform in platforms:
            if not isinstance(platform, str) or not platform.strip() or platform.strip() != platform.lower():
                fail(
                    f"corpus {name}: the generator names {platform!r} as a platform, and"
                    f" platforms are spelled the way std::env::consts::OS spells them"
                )

    environment = generator.get("environment", {})
    if not isinstance(environment, dict):
        fail(f"corpus {name}: the generator's environment is not a table")
        return
    for key, value in sorted(environment.items()):
        if not isinstance(value, str):
            fail(f"corpus {name}: the generator's {key} is not a string")


# The files a corpus's bytes pass through on their way into the store. An
# override to the digest check would have to be readable from one of these, so
# these are the ones the rules below are enforced over.
CORPUS_PATH = (
    "crates/bench-corpus/src/fetch.rs",
    "crates/bench-corpus/src/generate.rs",
    "crates/bench-corpus/src/store.rs",
    "crates/bench-cli/src/corpus.rs",
)

# Names an override would arrive under. This is a list of spellings rather than
# a rule, because the thing being prevented is somebody adding one in a hurry
# and a hurry does not invent new vocabulary.
OVERRIDE_NAMES = (
    "force",
    "no_verify",
    "no-verify",
    "skip_verify",
    "skip-verify",
    "skip_digest",
    "skip-digest",
    "allow_mismatch",
    "allow-mismatch",
    "ignore_digest",
    "ignore-digest",
    "insecure",
    "unverified",
)


OVERRIDE = re.compile(r"(?<![\w-])(?:" + "|".join(OVERRIDE_NAMES) + r")(?![\w-])")

# env!() is a compile time constant and std::env::consts::OS is a fact about the
# build, so neither is a way for a run to be told to behave differently. A read
# of the process environment is, and it is the one an override reaches for
# first, because it leaves no trace in the command somebody pastes into an issue.
ENVIRONMENT = re.compile(r"\benv::var(?:_os)?\b|\boption_env!")

# Every byte of a corpus enters the store through a call that names the digest
# the manifest pinned. insert_bytes and insert_file name whatever they are
# given, which is the right behaviour for a store and the wrong one for a
# corpus, so reaching for either of them here is how the check would get skipped
# without anybody having to write the word skip.
INSERT = re.compile(r"\binsert_(\w+)\b")
VERIFYING_INSERTS = {"stream", "verified"}


def check_source_for_overrides(name: str, text: str) -> None:
    """Reads one file of the corpus path and complains about anything that could skip a digest.

    Tests live at the end of every file in this workspace, so everything from
    the first `#[cfg(test)]` onwards is left alone. A test is allowed to say the
    words a run may not act on, and the test that proves a mismatch fails is
    going to say several of them.
    """
    lines = text.splitlines()
    end = next(
        (number for number, line in enumerate(lines) if line.strip() == "#[cfg(test)]"),
        len(lines),
    )
    inserts = name.endswith(("fetch.rs", "generate.rs"))
    for number, line in enumerate(lines[:end], 1):
        if line.lstrip().startswith("//"):
            continue
        for pattern, what in ((OVERRIDE, "an override"), (ENVIRONMENT, "a read of the environment")):
            hit = pattern.search(line)
            if hit:
                fail(
                    f"{name}:{number}: {hit.group()} looks like {what}, and a digest mismatch is a"
                    f" hard failure with no way around it"
                )
        if not inserts:
            continue
        for hit in INSERT.finditer(line):
            if hit.group(1) not in VERIFYING_INSERTS:
                fail(
                    f"{name}:{number}: {hit.group()} puts bytes in the store without naming the"
                    f" digest the manifest pinned"
                )


def check_digest_is_not_overridable() -> None:
    """A digest mismatch is a hard failure, and that is enforced rather than intended.

    None of this is true today by accident, it is true because the code was
    written that way. But a rule that is only true until somebody changes their
    mind is not a rule, and the change that would break it is a small one made
    at the end of a long afternoon.
    """
    for name in CORPUS_PATH:
        path = ROOT / name
        if not path.is_file():
            fail(f"{name}: is named as part of the corpus path and is not there")
            continue
        check_source_for_overrides(name, path.read_text(encoding="utf-8"))


def check_no_machine_identity() -> None:
    """Results carry hardware classes, never machine identity.

    Patterns here are shapes rather than values, so that adding a machine to the
    fleet does not mean editing this list.
    """
    patterns = [
        (re.compile(r"\b(?:\d{1,3}\.){3}\d{1,3}\b"), "an IP address"),
        (re.compile(r"\bssh\s+[a-z0-9_-]+@"), "an ssh target"),
    ]
    allow = re.compile(r"\b(?:0\.0\.0\.0|127\.0\.0\.1|255\.255\.255\.255|1\.1\.1\.1)\b")
    skip_dirs = {".git", "target", "corpus", "results", "store"}
    for path in ROOT.rglob("*"):
        if not path.is_file() or path.suffix not in {".md", ".rs", ".toml", ".yml", ".yaml", ".py", ".sh"}:
            continue
        if set(path.relative_to(ROOT).parts) & skip_dirs:
            continue
        if path.name == "discipline.py":
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        for line_no, line in enumerate(text.splitlines(), 1):
            for pattern, what in patterns:
                for hit in pattern.finditer(line):
                    if allow.fullmatch(hit.group()):
                        continue
                    rel = path.relative_to(ROOT)
                    fail(f"{rel}:{line_no}: looks like {what}: {hit.group()}")


def main() -> int:
    check_drivers()
    check_corpora()
    check_digest_is_not_overridable()
    check_no_machine_identity()
    if failures:
        print("Discipline checks failed:\n", file=sys.stderr)
        for failure in failures:
            print(f"  {failure}", file=sys.stderr)
        print(
            "\nThese checks exist because the method is only real if it is mechanised.",
            file=sys.stderr,
        )
        return 1
    print("Discipline checks passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
