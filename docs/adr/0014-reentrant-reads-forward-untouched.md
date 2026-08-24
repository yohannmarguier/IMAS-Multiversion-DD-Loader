# A seam that re-enters the shim mid-call is forwarded untouched

**Any** seam entered while this thread is already inside one is forwarded to IMAS-Core exactly as received: no registry lookup, no conversion-map resolution, no value transformation, no loss retention. It applies to every data-path seam — read, write and delete, ordinary and `al_plugin_*` alike — and to any depth beyond the first.

The rule was first written for reads only, and the reasoning below is the reasoning it was written with. That reasoning was right; the *set of seams* it was applied to was drawn too small. The widening is recorded under "The set of seams that enter the guard" further down. The file name still says `reentrant-reads`, and the same argument as ADR 0002 applies: every cross-reference names this file, so the name stays and the title carries the correction.

The reason is that such a call is not the caller this shim converts for. The shim converts a path once, at the boundary where an HLI hands it a path in the HLI's DD version. By the time the shim has called IMAS-Core, the path in flight is already in the *stored* DD version, and anything arriving from underneath that call is either IMAS-Core re-entering the public ABI or a plugin operating below the shim — in the stored DD world either way. Converting it again resolves a stored path as though it were an HLI one, which is wrong three times over:

- **The path.** A second resolution can move the path again, or claim it under a different rule. That it happens to be idempotent for one rule is luck, not a property: `b_tor -> b_field_phi -> b_field_phi` survives, while a rename whose destination is itself claimed by another rule does not.
- **The value.** A second `sign_flip` on the same buffer restores the original signs and hands the caller silently wrong data — the exact failure mode ADR 0010's "the shim therefore cannot apply a sign change twice" exists to forbid.
- **The loss log.** A reentrant read retains an entry the caller never earned, so the log reports fidelity loss on paths the HLI never asked for (ADR 0012).

## Why the depth of the call, and not the identity of the buffer

An earlier implementation keyed "have I already transformed this?" on the address of the returned allocation, comparing `*data as usize` against the last transformed pointer. That guard was too narrow and too wide at once. Too narrow, because it protected only the value transformation: the second path resolution and the spurious loss entry still happened, and the loss log is where that surfaced. Too wide, because an allocator is free to return a previously-seen address for a genuinely different buffer, and the guard would then skip a required sign flip — trading a double flip for a missing one, with the same silently-wrong-data outcome.

Call depth is the property that actually distinguishes the two cases, it needs no allocator assumptions, and it is one counter. The depth is thread-local: it describes one call stack, and ADR 0003 already places concurrent use of a single IMAS-Core context out of scope.

## The set of seams that enter the guard

The original rule counted depth inside the read seams only. Two facts about IMAS-Core show that set is too small.

**IMAS-Core re-enters on writes too.** Inside `al_write_data`, IMAS-Core calls its own public `al_plugin_write_data` with the same field pointer and the same buffer — the exact analogue of the read case this ADR was written for. So the counter must cover write and delete as well, and it must be **one** counter rather than one per family: a read arriving from underneath a write would find a read-only counter at zero and resolve an already-stored path as though it were an HLI path.

**IMAS-Core re-enters through seams that carry no DD path at all.** `al_write_plugins_metadata`, `al_bind_readback_plugins` and `al_unbind_readback_plugins` are public IMAS-Core exports; the shim mirrors all three and forwards them unchanged. From underneath them, IMAS-Core's plugin manager issues `al_plugin_read_data` and `al_plugin_write_data` against `ids_properties/plugins/**` and `ids_properties/homogeneous_time`. A guard scoped to the data-path family is at zero when that happens, so the shim would apply conversion policy to IMAS-Core's own housekeeping as though an HLI had asked for it.

So the guard is a **reentry guard**, not a read guard and not a data-path guard: every seam from which IMAS-Core can call back enters it, which is the data-path family plus those three. This is a wider reading of the same rule, not a second mechanism — there is still one thread-local counter and one condition.

Why this matters more than a wrong translation: IMAS-Core does not check the status it gets back from those calls. `AccessLayerPluginManager::write_field` ends with `assert(status.code == 0)`, and `bind_readback_plugins` does the same for its reads. For that caller a refusal is not a returned failure, it is `abort()`. Keeping IMAS-Core's own traffic out of the conversion path is therefore what makes ADR 0016's refusals safe to introduce at all; ADR 0019 decision 5 records the residual exposure.

For the supplied 3.39.0⇄4.1.1 artifact none of this is currently reachable: nothing claims `ids_properties/**` except the document-level identity default, so every such call resolves to the same path and forwards. The exposure is structural and waits for the first artifact with a rule under that subtree.

## What this does not change

A plugin that reads through `al_plugin_read_data` at top level — not underneath an in-flight read — is a first-level caller and still gets the full conversion policy that ADR 0002 and the plugin reentry seam specify. Every existing plugin-seam scenario drives the seam that way, and its behaviour is unchanged.

Version-stamp discovery (`src/version/version_stamp.rs`) does not go through the converting wrapper at all. `interpose::open_occurrence` injects the same raw `al_read_data` binding into its stamp reader, with none of the ordinary read policy, because that read is the shim's own and is what *decides* whether conversion applies to the occurrence: subjecting it to conversion would re-enter the layer from inside the code that supplies its input. The injected call enters the depth counter for its entire Core call, so a read arriving from underneath it is reentrant by this rule rather than by an ordering accident.

## Consequences

The shim's read policy no longer depends on how the platform resolves symbols. IMAS-Core's internal call to its own public `al_read_data` binds to the shim's exported definition on ELF, but not under macOS's two-level namespace, so before this rule the same read produced a different loss log and a different number of value transformations on Linux than on macOS. That divergence was found by pinning the loss-entry count in `tests/equilibrium_read_test.c` (the count is shim-owned, so it is platform-independent by construction) and is now covered platform-independently: the recording stub can be armed to re-enter the shim mid-read, which reproduces on any platform what real IMAS-Core only does on ELF.

The widened set needs the same treatment, and it is the only way it can be tested: macOS's two-level namespace means none of the write-side or plugin-metadata reentry can ever be reproduced there by real IMAS-Core, so a green local run proves nothing about it. Each newly-covered seam needs a stub knob mirroring the existing mid-read one — a write that re-enters, and a plugin-metadata seam that issues plugin reads and writes from underneath itself.
