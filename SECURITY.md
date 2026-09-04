# Security policy

## Reporting a vulnerability

Report privately through [GitHub Security Advisories](https://github.com/tamnd/iris-bench/security/advisories/new). Please do not open a public issue for a vulnerability.

You should get an acknowledgement within three working days.

## What counts as a vulnerability here

This tool downloads archives from the internet and unpacks them, by design. That is where the interesting failures are.

In scope:

- Path traversal or symlink escape when extracting a corpus archive, so that unpacking writes outside the corpus directory.
- A digest check that can be skipped, cached incorrectly, or made non fatal without an obviously named setting. Content addressing is the only thing standing between a mirror and a supplied corpus, so a way around it is a real problem.
- Decompression bombs that exhaust memory or disk without the declared limits stopping them.
- Command injection through a corpus name, a manifest field, or a driver configuration value that reaches a shell.
- Credentials for object storage endpoints leaking into result rows, environment captures, reproduction bundles, or logs.
- A reproduction bundle that includes something it should not, such as a token or a local path that identifies a machine.

Out of scope:

- A benchmark producing a wrong number. That is a correctness bug and it is arguably the most important kind of bug in this repository, but it is an issue rather than an advisory.
- Vulnerabilities in the systems under test. Report those to their maintainers, then tell us so we can bump the pin.
- Resource exhaustion from a workload that was asked for. Running TPC-H at a large scale factor is supposed to use the machine.

## A note on machine identity

Results are published with hardware classes and never with hostnames, addresses or account identifiers. If you find one of those in a published artifact, a reproduction bundle or an environment capture, report it. That is a leak, not a cosmetic issue.
