# Pin IMAS-Core until upstream corrects delete

ADR 0022 deliberately floated IMAS-Core in HLI validation so a green run
demonstrated that the shim still sat transparently in front of today's
upstream IMAS-Core. That premise is no longer safe. Upstream's HDF5
`deleteData` ignores its path and destroys the IDS occurrence (#139), so an
upstream move can change a destructive behaviour underneath the shim without a
change in this repository.

## Decision

1. CI pins IMAS-Core to the committed `IMAS_CORE_REF` in this repository's
   fork. The `full` job in `.github/workflows/ci.yml` downloads and tests that
   commit; `.github/workflows/hli-validation.yml` passes the same commit and
   fork repository to the IMAS-Fortran build.
2. The first pinned commit is behaviourally identical to the public 5.7.1
   release. Therefore, a later behavioural change is attributable to moving
   the pin, rather than to the decision to pin it.
3. This is temporary policy. Restore IMAS-Core floating when the delete
   correction lands upstream.

## Consequences

- CI no longer detects divergence between the fork and today's upstream
  IMAS-Core. No test replaces that lost upstream-tracking signal; a green run
  proves the shim against the pinned fork only.
- The pin makes a run reproducible and prevents an upstream change from
  silently altering the delete behaviour exercised by CI.
- ADR 0022's IMAS-Core row is superseded in place. Its Data Dictionary and HLI
  rows remain in force.

## Follow-up — 2026-09-11

The pin moved to `dae4abdd9428bd28f47063f8f575bdc8abd915f2`, which includes
IMAS-Core #64's path-aware HDF5 delete correction. The delete oracle now
requires both mapped datasets to disappear while unrelated values and the
stamp survive. The initial pin's behavior described above is historical.

The fork initially had no release tags. Core's `git describe` consequently
fell back to `0.0.0`, which the shim's major-version gate correctly refused.
The original upstream 5.7.1 and 5.7.2 tags were restored in the fork without
changing their objects or moving the source pin; the new pin reports
`5.7.2.86`. Both CI dependency cache keys were advanced to discard builds
configured without those tags. The runtime compatibility gate remains intact.

## Follow-up — 2026-09-20

The pin moved again to `3e5871a844c594491ab9e5365b63576f552bf50f`, which retains
the delete correction and adds IMAS-Core #66's absolute HDF5 field-read
correction. The issue #212 live coexistence oracle now requires direct-Core and
shim-converted absolute reads beneath an array-structure context to return the
same seeded value as the relative spelling in both conversion directions.
