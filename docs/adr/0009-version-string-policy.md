# The shim accepts only known DD releases and exact development DD-version stamps

The shim compares the HLI DD version of the calling binary with the stored DD version from an IDS occurrence's DD-version stamp before it creates a context record. It accepts a released version only when its `MAJOR.MINOR.PATCH` spelling names a version in the known DD version chain. It also accepts a development version only as `MAJOR.MINOR.PATCH-N-gHASH`: the base release must be in that chain, `N` is a non-zero decimal integer, and `HASH` is 7 to 64 lowercase hexadecimal characters. There is no whitespace, prefix, or extra suffix. This permits a real DD such as `4.1.1-47-g8eaa5f1` without pretending that it is the same DD as `4.1.1`.

Released versions compare as numeric triples. A development version is later than its base release, as the HLI ecosystem treats it, but the shim does not select a conversion from a Git hash. Two development versions are equal only when their complete strings are identical; every other development-version pair is refused as unmappable. Thus a calling binary and stored IDS occurrence at the same development version pass through unchanged, while `4.1.1` and `4.1.1-47-g8eaa5f1` do not silently pass through as a match.

All other strings are invalid: a dirty suffix, a PEP 440 form, a bare Git hash, `-1`, and arbitrary text. The shim rejects an invalid HLI DD version when it is set, or at the first seam when it comes from the environment. It rejects an invalid DD-version stamp immediately after the discovery read, before it enters the context registry. An invalid stamp is not an unstamped IDS occurrence: absence means the stamp is missing, while an invalid present value is unsafe storage metadata and must fail loudly.

## Considered Options

- **Snap a development version to its base release** — rejected. It would make a development DD appear to match a released DD and could return unconverted data through the shim.
- **Accept any semver-like or Git-derived string** — rejected. The DD-version stamp is unvalidated and no conversion-map artifact can identify an arbitrary value safely.
- **Treat an invalid stamp as absent** — rejected. It turns corrupt or unsupported metadata into silent identity forwarding.
