#!/usr/bin/env python3
"""Checks that are about discipline rather than about compilation.

The method in docs/METHODOLOGY.md is only real if it is mechanised. This script
is the mechanisation. It runs in CI on every change and it fails the build.

What it checks:

  1. Every driver in drivers/ has a CONFIG.md with all its required sections.
  2. Every driver's configuration review date is present and parseable, and is
     not more than 180 days old.
  3. Every corpus manifest declares a source, a licence, a category, and at
     least one digest, and every corpus in the mirror category has a licence
     note saying what permits us to redistribute it.
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
    corpora = ROOT / "corpora"
    if not corpora.exists():
        return
    for manifest_path in sorted(corpora.glob("*/manifest.toml")):
        name = manifest_path.parent.name
        manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
        corpus = manifest.get("corpus", {})
        for field in ("source", "licence", "category"):
            if not corpus.get(field):
                fail(f"corpus {name}: manifest has no '{field}'")
        if not manifest.get("files"):
            fail(f"corpus {name}: manifest declares no files, so nothing is pinned")
        for entry in manifest.get("files", []):
            if not entry.get("blake3"):
                fail(f"corpus {name}: file {entry.get('path')} has no digest")
        if corpus.get("category") == "mirror" and not corpus.get("licence_note"):
            fail(f"corpus {name}: mirrored without a licence note saying what permits it")


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
