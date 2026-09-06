#!/usr/bin/env python3
"""Tests for the manifest validator in discipline.py.

There are no corpora in the tree yet, so without this the manifest rules are
code that CI runs zero times and everybody assumes works. The first manifest to
arrive should meet a checker that has been checked, rather than being the thing
that finds out whether the checker is right.

Run with `python3 ci/test_discipline.py`. No test framework, because adding a
dependency to make five assertions readable is a bad trade in a file whose whole
job is to run everywhere without setup.
"""

from __future__ import annotations

import pathlib
import sys
import tempfile

import discipline

GOOD = """
[corpus]
name = "example"
description = "A corpus that exists to be validated"
source = "https://example.invalid/example.parquet"
licence = "Apache-2.0"
category = "fetch"

[[files]]
path = "example.parquet"
blake3 = "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
bytes = 1024
"""

GENERATED = """
[corpus]
name = "example"
description = "A corpus that exists to be validated"
source = "example-gen 1.0.0, scale factor 1"
licence = "Apache-2.0"
category = "generate"

[generator]
program = "example-gen"
arguments = ["-s", "1"]
version = "1.0.0"
version_arguments = ["-h"]

[generator.environment]
OUT = "{output}"

[[files]]
path = "example.tbl"
blake3 = "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
bytes = 1024
"""

failures: list[str] = []


def validate(text: str, directory: str = "example") -> list[str]:
    """Runs the manifest checks over one manifest and returns what they said."""
    with tempfile.TemporaryDirectory() as root:
        corpus = pathlib.Path(root) / directory
        corpus.mkdir()
        (corpus / "manifest.toml").write_text(text, encoding="utf-8")
        before = list(discipline.failures)
        discipline.failures.clear()
        try:
            discipline.check_manifest(corpus / "manifest.toml")
            return list(discipline.failures)
        finally:
            discipline.failures[:] = before


def expect_clean(name: str, text: str, directory: str = "example") -> None:
    said = validate(text, directory)
    if said:
        failures.append(f"{name}: expected no complaint and got {said}")


def expect_complaint(name: str, text: str, phrase: str, directory: str = "example") -> None:
    said = validate(text, directory)
    if not any(phrase in one for one in said):
        failures.append(f"{name}: expected a complaint containing {phrase!r} and got {said}")


def parts(manifests: dict[str, str]) -> list[str]:
    """Runs the subset check over a set of manifests and returns what it said."""
    import tomllib

    before = list(discipline.failures)
    discipline.failures.clear()
    try:
        discipline.check_parts({name: tomllib.loads(text) for name, text in manifests.items()})
        return list(discipline.failures)
    finally:
        discipline.failures[:] = before


def check_parts_cases() -> None:
    whole = GOOD.replace('"example"', '"whole"') + (
        '\n[[files]]\npath = "other.parquet"\n'
        'blake3 = "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3263"\n'
        "bytes = 2048\n"
    )
    part = GOOD.replace('category = "fetch"', 'category = "fetch"\npart_of = "whole"')

    said = parts({"whole": whole, "example": part})
    if said:
        failures.append(f"a real subset: expected no complaint and got {said}")

    said = parts({"example": part})
    if not any("not in corpora/" in one for one in said):
        failures.append(f"a subset of nothing: expected a complaint and got {said}")

    strayed = part.replace("example.parquet", "elsewhere.parquet")
    said = parts({"whole": whole, "example": strayed})
    if not any("which is not in 'whole'" in one for one in said):
        failures.append(f"a subset naming a file the whole lacks: got {said}")

    drifted = part.replace("bytes = 1024", "bytes = 4096")
    said = parts({"whole": whole, "example": drifted})
    if not any("differently from 'whole'" in one for one in said):
        failures.append(f"a subset pinning a file differently: got {said}")

    said = parts({"whole": GOOD.replace('"example"', '"whole"'), "example": part})
    if not any("is not smaller" in one for one in said):
        failures.append(f"a subset the size of the whole: got {said}")

    circular = GOOD.replace('category = "fetch"', 'category = "fetch"\npart_of = "example"')
    said = parts({"example": circular})
    if not any("part of itself" in one for one in said):
        failures.append(f"a subset of itself: got {said}")


def main() -> int:
    expect_clean("a complete manifest passes", GOOD)
    check_parts_cases()

    expect_complaint(
        "a manifest that disagrees with its directory",
        GOOD,
        "lives in",
        directory="something-else",
    )
    expect_complaint(
        "a category that is not one of the three",
        GOOD.replace('"fetch"', '"borrow"'),
        "docs/LICENSING.md",
    )
    expect_complaint(
        "mirrored without a licence note",
        GOOD.replace('"fetch"', '"mirror"'),
        "licence note",
    )
    expect_clean(
        "mirrored with a licence note and somewhere it is served from",
        GOOD.replace('"fetch"', '"mirror"\nlicence_note = "Public domain"').replace(
            "bytes = 1024", 'bytes = 1024\nurl = "https://ours.invalid/example.parquet"'
        ),
    )
    expect_complaint(
        "mirrored without saying where the bytes are served from",
        GOOD.replace('"fetch"', '"mirror"\nlicence_note = "Public domain"'),
        "nothing says where",
    )
    expect_complaint(
        "no files at all",
        GOOD.split("[[files]]")[0],
        "nothing is pinned",
    )
    expect_complaint(
        "a digest that is not lower case hex",
        GOOD.replace("af1349b9", "AF1349B9"),
        "BLAKE3 digest",
    )
    expect_complaint(
        "a digest of the wrong length",
        GOOD.replace(
            "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262", "abcdef"
        ),
        "BLAKE3 digest",
    )
    expect_complaint(
        "a path that climbs out of the corpus",
        GOOD.replace('"example.parquet"', '"../../etc/passwd"'),
        "inside the corpus directory",
    )
    expect_complaint(
        "a file declared as zero bytes",
        GOOD.replace("bytes = 1024", "bytes = 0"),
        "declares no size",
    )
    expect_complaint(
        "the same path listed twice",
        GOOD + GOOD[GOOD.index("[[files]]") :],
        "listed twice",
    )
    expect_complaint(
        "a missing description",
        GOOD.replace('description = "A corpus that exists to be validated"', ""),
        "'description'",
    )
    expect_complaint(
        "a fetched corpus with nowhere to fetch from",
        GOOD.replace('"https://example.invalid/example.parquet"', '"tpch-dbgen -s 20"'),
        "nowhere to fetch it from",
    )
    expect_clean(
        "a fetched corpus whose entry says where its bytes are",
        GOOD.replace('"https://example.invalid/example.parquet"', '"tpch-dbgen -s 20"').replace(
            "bytes = 1024", 'bytes = 1024\nurl = "https://example.invalid/example.parquet"'
        ),
    )
    expect_clean("a generated corpus that says what produces it", GENERATED)
    expect_complaint(
        "a generated corpus with no generator",
        GOOD.replace('"fetch"', '"generate"'),
        "nothing says how",
    )
    expect_complaint(
        "a fetched corpus that has a generator anyway",
        GENERATED.replace('"generate"', '"fetch"'),
        "two different answers",
    )
    expect_complaint(
        "a generated corpus whose source is a download",
        GENERATED.replace(
            '"example-gen 1.0.0, scale factor 1"', '"https://example.invalid/example.tbl"'
        ),
        "produced here and that they are downloaded",
    )
    expect_complaint(
        "a generator whose version cannot be probed",
        GENERATED.replace('version_arguments = ["-h"]', "version_arguments = []"),
        "cannot be checked",
    )
    expect_complaint(
        "a generator with no version at all",
        GENERATED.replace('version = "1.0.0"', ""),
        "'version'",
    )
    expect_complaint(
        "a generator field this format does not have",
        GENERATED.replace("version =", 'versions = "nearly right"\nversion ='),
        "'versions'",
    )
    expect_complaint(
        "a generator environment that is not strings",
        GENERATED.replace('OUT = "{output}"', "OUT = 3"),
        "is not a string",
    )
    expect_complaint(
        "a manifest that is not TOML",
        "this is not toml at all {{",
        "not valid TOML",
    )
    expect_complaint(
        "a field this format does not have",
        GOOD.replace("licence =", "licence_notes = \"nearly right\"\nlicence ="),
        "'licence_notes'",
    )

    if failures:
        print("Validator tests failed:\n", file=sys.stderr)
        for failure in failures:
            print(f"  {failure}", file=sys.stderr)
        return 1
    print("Validator tests passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
