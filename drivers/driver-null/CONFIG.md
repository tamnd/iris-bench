# driver-null configuration

## Source

Not applicable. This driver does no work, so there is nothing to configure and nothing to get wrong. It exists so that running the harness against a system that does nothing measures what the harness itself costs.

It implements the trait in full and returns an empty result from every query. That means every digest it produces disagrees with every real system, which is the correct outcome for a driver that computed nothing, and it is why this driver never appears in a results table.

## Settings

None.

## Deviations

None.

## Rejected alternatives

Measuring instrumentation overhead by subtracting two real systems was considered and rejected. It confounds the harness cost with whatever the two systems happen to differ by, which is the thing being measured.

## Last reviewed

2026-09-06
