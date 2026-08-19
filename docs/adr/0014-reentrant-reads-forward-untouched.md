# A read that re-enters the shim mid-read is forwarded untouched

A read seam entered while this thread is already inside one is forwarded to IMAS-Core exactly as received: no registry lookup, no conversion-map resolution, no value transformation, no loss retention. This applies to `al_read_data` and `al_plugin_read_data` alike, since both are the same `read_data_impl`, and to any depth beyond the first.

The reason is that such a call is not the caller this shim converts for. The shim converts a path once, at the boundary where an HLI hands it a path in the HLI's DD version. By the time the shim has called IMAS-Core, the path in flight is already in the *stored* DD version, and anything arriving from underneath that call is either IMAS-Core re-entering the public ABI or a plugin operating below the shim — in the stored DD world either way. Converting it again resolves a stored path as though it were an HLI one, which is wrong three times over:

- **The path.** A second resolution can move the path again, or claim it under a different rule. That it happens to be idempotent for one rule is luck, not a property: `b_tor -> b_field_phi -> b_field_phi` survives, while a rename whose destination is itself claimed by another rule does not.
- **The value.** A second `sign_flip` on the same buffer restores the original signs and hands the caller silently wrong data — the exact failure mode ADR 0010's "the shim therefore cannot apply a sign change twice" exists to forbid.
- **The loss log.** A reentrant read retains an entry the caller never earned, so the log reports fidelity loss on paths the HLI never asked for (ADR 0012).

## Why the depth of the call, and not the identity of the buffer

An earlier implementation keyed "have I already transformed this?" on the address of the returned allocation, comparing `*data as usize` against the last transformed pointer. That guard was too narrow and too wide at once. Too narrow, because it protected only the value transformation: the second path resolution and the spurious loss entry still happened, and the loss log is where that surfaced. Too wide, because an allocator is free to return a previously-seen address for a genuinely different buffer, and the guard would then skip a required sign flip — trading a double flip for a missing one, with the same silently-wrong-data outcome.

Call depth is the property that actually distinguishes the two cases, it needs no allocator assumptions, and it is one counter. The depth is thread-local: it describes one call stack, and ADR 0003 already places concurrent use of a single IMAS-Core context out of scope.

## What this does not change

A plugin that reads through `al_plugin_read_data` at top level — not underneath an in-flight read — is a first-level caller and still gets the full conversion policy that ADR 0002 and the plugin reentry seam specify. Every existing plugin-seam scenario drives the seam that way, and its behaviour is unchanged.

Version-stamp discovery (`src/version_stamp.rs`) does not go through the converting wrapper at all. `interpose::open_occurrence` injects the same raw `al_read_data` binding into its stamp reader, with none of the ordinary read policy, because that read is the shim's own and is what *decides* whether conversion applies to the occurrence: subjecting it to conversion would re-enter the layer from inside the code that supplies its input. The injected call enters the depth counter for its entire Core call, so a read arriving from underneath it is reentrant by this rule rather than by an ordering accident.

## Consequences

The shim's read policy no longer depends on how the platform resolves symbols. IMAS-Core's internal call to its own public `al_read_data` binds to the shim's exported definition on ELF, but not under macOS's two-level namespace, so before this rule the same read produced a different loss log and a different number of value transformations on Linux than on macOS. That divergence was found by pinning the loss-entry count in `tests/equilibrium_read_test.c` (the count is shim-owned, so it is platform-independent by construction) and is now covered platform-independently: the recording stub can be armed to re-enter the shim mid-read, which reproduces on any platform what real IMAS-Core only does on ELF.
