# Delete losses make destructive fan-out visible

ADR 0017 correctly says a delete must fan out over every stored candidate: a
delete asserts absence, and leaving a fallback candidate behind would let a
later read serve stale data. Its former consequence that the delete seam never
retains a loss is superseded here. A successful fan-out is faithful at the
path-conversion level, but IMAS-Core's HDF5 backend ignores each path and can
delete the whole occurrence on the first call. A caller needs evidence of that
destructive risk in the same two loss channels as reads and writes.

## Decisions

1. A refused delete retains one `UNMAPPABLE` `DELETE` loss naming the caller's
   complete HLI-DD path before it returns its ordinary refusal.
2. After a candidate-plan delete has attempted every stored candidate, it
   retains one `POTENTIALLY_LOSSY` `DELETE` loss per visited stored path. This
   happens even when the first candidate failure is returned: later candidates
   are still attempted, and the loss records what the shim did.
3. A one-path delete remains exact and retains nothing. The delete policy
   returns the visited stored candidates as its completion verdict; the
   interposition layer alone attaches occurrence identity, global registry
   retention and file delivery.
4. `IMAS_MVDD_LOSS_OPERATION_DELETE` is the third operation value. It is a
   crate-root constant that cbindgen emits as a C preprocessor definition, not
   a fifth shim-owned export.

## Consequences

- The in-memory context log and append-only loss-log file both expose delete
  loss with operation word `delete`.
- The file is the durable evidence trail for HDF5's whole-occurrence delete
  exposure (#139), even if the root context ends before a caller drains it.
- ADR 0017's fan-out and no-probe decisions remain unchanged; this ADR only
  makes their destructive effect observable.
