# Read-path seam policy

The shim must apply one explicit policy at each ABI seam. This policy keeps the
IDS name as a stable logical key, discovers the stored DD version for each IDS
occurrence, and translates DD paths only where a context supplies that version.
It prevents path conversion from being scattered across individual forwarding
functions.

This ADR originally also refused every write across a known DD-version
difference. That is no longer the policy: ADR 0016 replaces it with a
best-effort write path, and ADR 0017 gives delete its own rules. The table row
below records where those decisions now live. The title stays as it is —
"read-path" undersells the content, but every cross-reference in `CLAUDE.md`,
the other ADRs and the source comments names this file, and the churn of a
rename outweighs the inaccuracy.

| ABI function | Shim action |
|---|---|
| All functions when `HLI_V` is unset | Forward unchanged. |
| `al_begin_dataentry_action` | Register the data-entry context only. It has no DD version. |
| `al_begin_global_action` | Forward the IDS name unchanged. Open the operation context, read its version stamp before returning to the HLI, then register the IDS occurrence. Translate `datapath` when the occurrence version is already known *and* the conversion map resolves it to a concrete stored path; on its first use, or when the map answers with no stored source or a refusal, forward it unchanged. |
| `al_begin_slice_action`, `al_begin_timerange_action` | Apply the same version-discovery and occurrence-registration rule as global action. Forward the IDS name unchanged. |
| `al_begin_arraystruct_action` | Translate `path` and `timebase` before calling IMAS-Core. On success, register the AoS context. |
| `al_iterate_over_arraystruct` | Forward unchanged. The registry stores no AoS current-element state. |
| `al_read_data` | Resolve and translate `field` and `timebase` when versions differ. Convert returned values before the HLI receives them. If no stored version is available, forward unchanged and do not convert. |
| `al_write_data` | **Superseded by ADR 0016.** Best effort: translate and write where it is safe, refuse before calling IMAS-Core where it is not, and never return success for data that was not stored. Where versions match, or no stored version is available, forward unchanged as before. |
| `al_delete_data` | **Superseded by ADR 0017.** Fan out over every resolved candidate, probing each one first; refuse a subtree delete with an escaping rule; forward the empty path unchanged. Where versions match, or no stored version is available, forward unchanged as before. |
| `al_end_action` | On success, remove only that context's record. Parent contexts do not own child-context lifetimes. |
| `al_close_pulse` | Forward unchanged. It releases no context ID and therefore does not mutate the registry. |
| `al_get_occurrences` | Forward unchanged. IDS names are stable. |
| `al_list_filled_paths` | Out of scope. Forward unchanged. |
| `al_bind_plugin`, `al_unbind_plugin` | Out of scope. Forward the field path unchanged because no IDS occurrence supplies a stored DD version. |
| Six linkable `al_plugin_*` reentry functions | Apply the matching non-plugin rules. |
| `al_plugin_begin_timerange_action` | Stay absent. It is not a usable IMAS-Core C symbol. |
| Every other IMAS-Core export | Plain forward. |

The context registry decides the exact keys and data structure. This policy only
defines when each seam reads, updates, or removes its state.

`datapath`'s third case is deliberate rather than an omission. A no-source or
refusal answer is not this seam's call to make: `datapath` is near-inert — five
of six backends ignore it outright, and only UDA in remote mode with
`cache_mode=ids` honours it — so refusing an open over it would deny the caller
an occurrence it can very likely still read, on the strength of an argument the
backend is about to discard. Forwarding it unchanged leaves the eventual
`al_read_data` on that occurrence as the one seam that reports the absence or
raises the refusal, where the caller has a path and a version pair to be told
about. Only a concrete resolved path is a basis for rewriting it.
