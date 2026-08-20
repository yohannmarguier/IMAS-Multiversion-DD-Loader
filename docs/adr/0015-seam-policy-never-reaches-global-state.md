# A seam policy never reaches process-global state

The shim's C ABI seams are split into two layers. The **interposition** layer (`src/interpose.rs`, with the runtime binding in `src/core/core_binding.rs`) owns everything that touches C or the process: `dlopen`/`dlsym`, raw pointers, `CString`, `al_status_t`, the HLI DD version latch, the read-depth counter, and the context registry. The **seam policy** layer (`src/conversion/seam_policy.rs`, over `src/conversion/path_conversion.rs`) owns the decisions: which arguments translate, which contexts refuse, what fidelity a read earned, what the shim registers afterwards.

The rule this ADR fixes is the direction of the dependency between them. A seam policy receives what it needs as values and returns the effects it wants performed. It never reads a global, never writes one, and never calls IMAS-Core directly.

Concretely, a seam policy function:

- receives the HLI DD version as a value — on a `ConversionRecord` snapshot for the data seams, as a parameter for the two occurrence-opening seams — and never calls `hli_version::latched()`;
- receives a `ConversionRecord` snapshot and never calls `ContextRegistry` itself;
- calls IMAS-Core only through a closure the interposition layer injects;
- returns the registry writes and the end-action call it wants, rather than performing them.

## Why the latch in particular

`src/version/hli_version.rs` states that its latch cannot be set from a unit test by design: `LATCH` is a process-wide `OnceLock`, `cargo test` runs every test in one process, and two tests exercising it would race for who latches it first. That is the correct design for a process-wide property (ADR 0005), and this ADR does not reopen it.

But it has a consequence that decides this one. If a seam policy reads the latch, **no `cargo test` test can ever choose a version**, so every policy behaviour must be proven through an isolated ctest process against the C ABI. That is how the code arrived at roughly ninety top-level functions reachable only from the C suite, and a 2790-line module with seventeen unit tests. Passing the version in is what makes the decisions reachable from `cargo test` at all.

The same argument applies to any process-global the policy might reach. The conversion-map cache already demonstrated the failure mode from the other side: a test fixture keyed on the real `equilibrium` 3.39.0/4.1.1 pair shared whichever map was already live for that pair, and failed roughly one run in 150 depending on which other test held a record at the time.

## Why effects are returned rather than performed

The loss log is the case that motivated the rule. Before the split, nine return points inside the read loop each called `retain_read_fidelity` on the way out. Issue #65 was a return path that forgot to call it, and issue #66 was the same nine sites each constructing the retained path, one of them without the arraystruct anchor prefix.

Returning a list of loss entries would have reproduced both: nine return points, each of which can still forget to push. So the verdict a seam policy returns carries the field and timebase fidelity as **mandatory struct fields**, and the interposition layer derives the log entries mechanically. A forgotten fidelity is a compile error, and the path is constructed in one place.

That generalises. When a decision and its effect are separated, the effect should be a value the decision cannot omit, not a call the decision must remember to make.

## What this does not change

ADR 0005 stands unchanged: the HLI DD version still latches once per process, through the shim-owned setter or the environment fallback, and the latch is still the production entry point. The interposition layer reads it once per seam and hands the value down. A parameter is not a second entry point.

ADR 0003 stands unchanged: the context registry still owns all conversion state and is still reached only through its own API. It gains no new caller — it loses one, because the seam policy no longer calls it.

ADR 0014's depth guard stays in the interposition layer even though its *reason* is a policy reason. It is a gate on whether the policy runs at all, in the same category as the `conversion_is_possible()` short-circuit, and it must run before the registry lookup that produces the snapshot the policy needs. Placing it in the policy would take the registry mutex on every read IMAS-Core issues internally.

## Consequences

The decisions this shim exists to make become testable without `dlopen`, without the recording stub, and without latching a version. The read loop's bookkeeping — the lines that carried both historical defects in this area — is reachable from `cargo test` for the first time.

The cost is that a seam policy no longer decides what lands in the loss log, only what the fidelity was, and no longer performs the registration it decided on. Both are bookkeeping, and both are now written in one place instead of nine.

This ADR does not, by itself, make a wrong wiring impossible. A future contributor can pass a policy the wrong snapshot, or perform a returned effect against the wrong context. What it removes is the ability to reach around the parameter list to a global, which is the failure the layering exists to prevent and the one a reviewer cannot see locally.
