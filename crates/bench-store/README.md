# bench-store

Append only result storage and the claim ledger.

Raw iterations are stored, never summaries. A store that kept only medians would be a store nobody could re-analyse with a different statistic later, and re-running is expensive enough that the choice of statistic has to stay open long after the machine has moved on to something else.

Nothing is deleted and nothing is edited. A correction is a new row that names the run it corrects, so a result that turned out to be wrong is still readable next to the thing that replaced it. That is what makes the published losses rule mean something: a number that can be quietly removed is a number nobody has to stand behind.

Part of [iris-bench](https://github.com/tamnd/iris-bench). Licensed under Apache-2.0.
