# bench-report

Rendering, including the rules that stop a misleading table being drawn.

Those rules are code in this crate rather than editorial policy, because a policy is something a person can be in a hurry about. A comparison whose confidence interval crosses one renders as no measurable difference rather than as a winner. A geometric mean never appears without the per query table it came from on the same page. Failed and unsupported queries are shown rather than dropped, since a mean over the queries a system could answer is a different number from a mean over the queries it was asked.

A result taken on a machine that failed its eligibility gates, or under a configuration this repository had to guess at, renders with that said next to it and not in a footnote. The marker travels with the number wherever the number goes.

Part of [iris-bench](https://github.com/tamnd/iris-bench). Licensed under Apache-2.0.
