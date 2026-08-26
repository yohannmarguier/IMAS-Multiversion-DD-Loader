# The HLI validation job floats IMAS-Core and pins the Data Dictionary

CI builds a real HLI — the IMAS-Fortran fork at `yohannmarguier/IMAS-Fortran` — against the installed shim with `AL_USE_MULTIVERSION_SHIM=ON`, and runs that HLI's own test suite. Of the three moving parts that job depends on, two are pinned and one is deliberately not:

| Dependency | Treatment | Why |
|---|---|---|
| IMAS-Core | **floats** — whatever the HLI's own default acquires | The shim's version gate is major-only, so a 5.x release is a `VersionDrift` log and not a failure. Pinning it would mean editing this repository for every IMAS-Core release, which is the opposite of what the shim claims to be. |
| Data Dictionary | **pinned to 4.1.1** | The shim ships exactly one conversion-map artifact, for 3.39.0 ⇄ 4.1.1. A different DD version does not weaken the conversion test; it dissolves it. |
| The HLI fork | **pinned by commit** in `IMAS_FORTRAN_REF` | Gives the build cache a content-derived key, and makes a red run attributable to the shim rather than to someone else's work in progress. |

The asymmetry is the decision. It looks inconsistent from a distance and is not.

## Why IMAS-Core floats

The shim's whole claim is that it drops into an existing HLI ⇄ IMAS-Core pipeline without either side being rebuilt for it. A CI job that pins IMAS-Core tests that claim against one frozen instant and needs a commit here every time IMAS-Core cuts a release. Floating it makes the job answer the question actually worth asking: does the shim still sit transparently in front of *today's* IMAS-Core?

This is only safe because of what `check_major_version` does (`src/core/core_binding.rs`). The shim compares the major component of the IMAS-Core version it was built against — baked in from `IMAS_CORE_VERSION` by `build.rs` — with what `getALVersion()` reports at run time. Equal majors with different minors produce a `VersionDrift`, which is recorded and does not stop the call. Different majors are a refusal. So the whole 5.x line is admissible without a code change, and the job's floating dependency has a hard, stated boundary rather than an open one.

The day upstream cuts 6.0.0, every test in the job fails at once with `IMAS-Core major version mismatch`. That is the correct signal — the shim genuinely does not support that Core yet — but it presents as a total collapse rather than as one fact. The job therefore writes the resolved IMAS-Core commit and the shim's own drift verdict into its step summary, so the failure can be read for what it is without opening a build log.

Note that the version the shim reports itself as built against still comes from this repository's `IMAS_CORE_VERSION` pin, which the `full` job uses for its real-Core suite. Floating applies to the Core the *HLI* acquires and the shim then opens, not to that pin. The two are allowed to differ, and when they do, the drift line in the summary is what says so.

## Why the Data Dictionary does not float

The same reasoning does not transfer, because the shim's coverage of DD versions is not a range. It is one pair. `src/known_artifacts.rs` embeds a single conversion-map artifact, `docs/3.39.0--4.1.1.xml`, and the fork's committed fixtures are HDF5 pulses written under exactly those two versions.

Suppose the DD floated and moved to 4.2.0. The HLI DD version becomes 4.2.0, the stored DD version of the fixture stays 3.39.0, no embedded artifact serves that pair, so `discover_and_register_occurrence` registers nothing (ADR 0007). Nothing converts. The cross-version test then fails — but for a reason that has nothing to do with the shim being wrong, and the failure would read identically to a genuine conversion regression.

Pinning the DD is therefore not bookkeeping to be revisited on a schedule. The pin moves when, and only when, a new conversion-map artifact exists to justify it. That makes it a statement about this repository's coverage, which is exactly what it should be.

## What the job proves, and what it does not

Two claims, from one build:

- **Passthrough.** The 83 generated per-IDS tests write and read every IDS across the memory, ASCII and HDF5 backends with the HLI DD version equal to the stored DD version. No conversion applies, so every value must return unchanged. This is a large volume of traffic through the read, write and delete seams — far more than the C ABI suite drives — and its only assertion is that the shim is not there.
- **Conversion.** `playground/play_eq_two_dd` reads the 3.39.0 fixture with a 4.1.1 HLI. Its `PASS_REGULAR_EXPRESSION` requires `grids_ggd/grid/space/coordinates_type` to be named, which is the `retyped` refusal (`RefusalReason::UnservableRetype`). A shim that silently stopped registering conversion records would print no refusal and fail that regex, so the assertion is a positive one.

Three things it does not prove, each worth stating because a green run invites the opposite assumption:

**The `examples/` I/O tests never run.** All twenty are gated on `(NOT AL_BACKEND_MDSPLUS OR NOT AL_BACKEND_HDF5)` — both backends, not either — and `common/cmake/ALCore.cmake` raises `FATAL_ERROR` if MDSplus is requested alongside the shim, because the tests need an `al-mdsplus-model` target that only a non-shim build creates. So those twenty are permanently disabled in any shim build, and their get/put/create paths are uncovered here. This is why the job pins its disabled count as well as its enabled one: the number is a standing statement about a gap, not an incidental total.

**A NAG build is not covered.** The fork's NAG branch hardcodes `-lal` rather than linking the `al` target, so it links IMAS-Core directly and silently defeats `AL_USE_MULTIVERSION_SHIM` — no error, and once conversion is involved, no symptom except data that was never translated. The job runs gfortran, so it says nothing about that path. The fork's own ADR 0001 and a warning at the branch itself carry the detail.

**Value correctness is not this job's claim.** The cross-version test tolerates differing and sign-flipped rows by design; whether a converted value is *right* belongs to `tests/real_core/equilibrium_read_test.c`, which owns the oracle. Asserting it here would put the fork in charge of this repository's expected values.

## Consequences

A red HLI job has exactly three causes, and they are distinguishable without bisecting: the shim changed behaviour at a seam, the pinned fork commit was moved, or upstream IMAS-Core moved. The first is the point of the job. The second is visible as a one-line diff to `IMAS_FORTRAN_REF` in the same pull request. The third is visible in the step summary, and is the only one that can appear in a pull request that did not touch either pin.

The pins are what make that true. A floating fork ref would collapse the first two causes together, and there would be no way to tell a shim regression from unrelated work in progress on the HLI side.

Caching follows from the same pins. The expensive part of the job — building the Data Dictionary and compiling roughly a million lines of generated Fortran — depends on the DD version and the fork commit, and not on the shim: the HLI links the shim as an interface and opens IMAS-Core through it at run time, so a shim change requires a relink and a test run, not a rebuild. Keying the build tree on the two pins plus the resolved Core commit is therefore both correct and worth about thirty minutes a run. It also means the cache invalidates when upstream Core moves, which is when the Core half of the job's claim genuinely needs re-establishing.

**Correction (cache scope and restore keys).** The reasoning above is about what the key should *contain*, and it stands. What it missed is where a cache can be *read from*: a GitHub Actions cache is visible only to the ref that wrote it and to the repository's default branch. This workflow triggered on `pull_request` alone, so every cache it wrote was scoped to a single `refs/pull/N/merge` and no other pull request could ever read it — the first run saved a 350 MB tree faithfully and no run has used it since. Building on push to `main` and `develop` is what gives the cache a shared scope to live in, and it is the reason the job was still paying twenty-two minutes a run for a mechanism that was, on paper, already in place.

The same run showed the second cost of the exact-key-only rule. It was chosen so that a miss produced a clean build rather than a tree half-built against different sources, but it priced every miss at the full cold build — and Core floats by design, so upstream moving Core discarded the whole tree over a dependency that rebuilds in minutes. There is now a restore-keys ladder, and the hazard the rule guarded against is handled where it occurs: an incremental build that fails off a restored tree discards it and builds once from clean.

