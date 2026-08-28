# A refusal is only safe if someone catches it

ADR 0016 makes the write path refuse wherever it cannot write safely. A refusal is only a *good* answer if the caller survives it. This ADR records who is expected to catch a refusal, why that expectation is split across two repositories, and the one caller for whom a refusal is not survivable at all. It was a decision record only when written; issue #126 implements its shim-side half, and issue #134 filed its HLI-side half as `yohannmarguier/IMAS-Fortran#61` and stated the residual limitation in README.md rather than coding around it.

## The problem a refusal creates on the write path

On the read path a refusal costs one field. The generated Fortran read routines already tolerate it: `is_external_refusal` in the IMAS-Fortran fork's `al_get_policy` recognises the shim's reserved code range, records the path, code and message in a skip log, and lets the read continue. That module's own header states the argument — the path does not exist in the caller's dictionary and no retry can produce it, so aborting the whole `ids_get` costs every other field for nothing.

On the write path the same refusal costs much more, and the tolerance is not there. The generated `isErrorCritical` takes the refusal-tolerance branch only when it is generated for a `get`; the `put` variant has neither the `use al_get_policy` nor the branch, so any non-zero status returns "critical" and the routine returns. There is no rollback and no transaction. Everything already written earlier in the DD traversal is on disk. So a refusal partway through a `put_slice` leaves a **torn time slice plus an error**, not a clean failure.

That matters because ADR 0016 decision 3's refusal lands inside `put_slice`: all 13 `right_only` paths in the supplied artifact sit under `time_slice`, the dynamic AoS.

## Decisions

1. **The shim refuses, and does not pretend to write.** The alternative — return `code == 0` and record a loss log entry — was rejected in ADR 0016 decision 3 and stays rejected. Making the *shim* lenient means it lies to every caller, including the ones that would have handled the truth.

2. **The tolerance belongs in the HLI, per field.** Extending the fork's read-side mechanism to `put` sites is the right fix: a refused write leaves that field unwritten, records it in the skip log, and the rest of the slice completes. The HLI is the only layer that knows whether an unset field is acceptable for what it is doing; the shim cannot know that and should not guess.

   The argument for tolerating is in fact *stronger* on the write path than on the read path. On a read, tolerating costs a field the caller could never have had. On a write, **refusing** costs every field after it in the traversal, half-written, on disk.

3. **Two things the extension owes.** The `al_get_policy` header's reasoning is written for reads and must be redone for writes: "leave the field unset and carry on" means something materially different when the field is a destination rather than a source. And its scope limit — that only `isErrorCritical` and the failure arm of `al_begin_arraystruct_action` may consult the predicate, never the `al_begin_global_action` or data-entry seams — must be restated for the put sites, for the same reason: the shim returns the same code for a malformed version stamp or a version-latch conflict, and tolerating one of *those* would sail past an IDS that was never opened.

4. **The shim must not require the extension.** This is a change in a repository the shim does not control, in a fork that is ahead of the pinned upstream. Against an unmodified upstream IMAS-Fortran, ADR 0016 decision 3's refusal tears the slice. That is a **documented limitation of this shim, not a defect in it** — and it must be documented, because a user whose `put_slice` half-completes will otherwise report it as a shim bug.

5. **One caller cannot survive a refusal at all, and this is accepted with eyes open.** IMAS-Core calls the shim's own exports from inside its plugin machinery and does not check the status it gets back: `AccessLayerPluginManager::write_field` ends with `assert(status.code == 0)`, and `bind_readback_plugins` does the same for its reads. For that caller a refusal is not a returned failure — it is `abort()`, and the process dies.

   For the supplied artifact the blast radius is empty. Nothing claims `ids_properties/plugins/**` except the document-level identity default, so every one of those calls resolves to the same path and forwards. The exposure is structural, and it waits for the first conversion-map artifact with a rule under `ids_properties/**`.

   Rejected: a carve-out refusing to ever refuse a path under `ids_properties/plugins/**`. It writes a DD subtree name into the shim to compensate for a guard that should have been wide enough, and it does not solve the general case, because IMAS-Core asserts on bound-plugin data writes too. ADR 0014's widened reentry guard is the real mitigation, since it stops the shim applying conversion policy to IMAS-Core's own calls in the first place.

## Considered Options

- **Make the shim lenient instead of the HLI** — rejected in decision 1. It removes the caller's ability to know.
- **Refuse the whole operation at the first unwritable field, deliberately and early** — rejected. It is the current behaviour by accident, and it is worse than it looks: the operation does not stop *before* writing, it stops *partway through*.
- **Buffer the writes in the shim and apply them only if all succeed** — rejected. It makes the shim a transaction manager over an ABI with no transaction, it needs unbounded memory proportional to the IDS, and it changes when the caller's data reaches disk.
- **Have the shim detect an intolerant HLI and change behaviour** — rejected. There is nothing to detect: the HLI's tolerance is a compile-time property of generated code the shim never sees.

## Consequences

- **The write path's correctness argument spans two repositories.** That is unusual enough to be the reason this ADR exists. The shim half is verifiable here; the HLI half is not, and no test in this repository can prove it.
- **Two failure modes must be documented for users, not just for maintainers.** Against a patched HLI, an unwritable field is skipped and reported. Against an unmodified one, the `put_slice` tears. Which one a user gets depends on which IMAS-Fortran they linked.
- **The IMAS-Fortran change needs its own issue in its own repository**, and this shim's work must not be blocked on it.
- **ADR 0014's guard widening is load-bearing, not housekeeping.** Decision 5 is only tolerable because the reentry guard keeps IMAS-Core's own plugin traffic out of the conversion path.
