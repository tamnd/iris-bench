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
    },
    "files": {"path", "blake3", "bytes", "url"},
    "assertions": {"rows", "columns"},
}

# A driver may depend on the trait crate and on nothing else in this workspace.
# Otherwise a driver can reach into the runner and special case itself, which is
# the single failure mode that would make every comparison here worthless.
DRIVER_ALLOWED_INTERNAL_DEPS = {"bench-driver"}

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
    for directory in sorted(p for p in corpora.iterdir() if p.is_dir()):
        manifest_path = directory / "manifest.toml"
        if not manifest_path.exists():
            fail(f"corpus {directory.name}: no manifest.toml")
            continue
        check_manifest(manifest_path)


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
