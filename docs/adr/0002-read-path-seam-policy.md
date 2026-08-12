# Read-path seam policy

The shim must apply one explicit policy at each ABI seam. This policy keeps the
IDS name as a stable logical key, discovers the stored DD version for each IDS
occurrence, translates DD paths only where a context supplies that version, and
refuses writes across known DD-version differences. It prevents path conversion
from being scattered across individual forwarding functions.

| ABI function | Shim action |
|---|---|
| All functions when `HLI_V` is unset | Forward unchanged. |
| `al_begin_dataentry_action` | Register the data-entry context only. It has no DD version. |
| `al_begin_global_action` | Forward the IDS name unchanged. Open the operation context, read its version stamp before returning to the HLI, then register the IDS occurrence. Translate `datapath` when the occurrence version is already known; on its first use, forward it unchanged. |
| `al_begin_slice_action`, `al_begin_timerange_action` | Apply the same version-discovery and occurrence-registration rule as global action. Forward the IDS name unchanged. |
| `al_begin_arraystruct_action` | Translate `path` and `timebase` before calling IMAS-Core. On success, register the AoS context. |
| `al_iterate_over_arraystruct` | Forward unchanged. The registry stores no AoS current-element state. |
| `al_read_data` | Resolve and translate `field` and `timebase` when versions differ. Convert returned values before the HLI receives them. If no stored version is available, forward unchanged and do not convert. |
| `al_write_data`, `al_delete_data` | If known versions differ, return failure without calling IMAS-Core. Otherwise forward unchanged. |
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
