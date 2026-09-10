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
