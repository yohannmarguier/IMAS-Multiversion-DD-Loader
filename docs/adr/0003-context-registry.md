# The context registry owns mismatched context state

The shim has one context registry for active contexts whose stored DD version differs from the HLI DD version. IMAS-Core uses one shared live context-ID space, so a raw context ID identifies at most one record; it can be reused after `al_end_action`. The registry does not store matching-version contexts; they and other unknown contexts pass through to IMAS-Core without conversion.

Each record stores its resolved absolute HLI DD path, its conversion-map reference, its pulse context ID, and, when applicable, its parent context ID. The record holds its own resolved data, so a read takes one lookup. A parent context helps construct a child record, but it does not own that child's lifecycle.

After a successful `al_end_action`, the registry removes only that context's record. `al_close_pulse` does not change the registry because it does not release an IMAS-Core context ID. Conversion maps are shared by `(IDS name, stored DD version, HLI DD version)` and exist only while one or more records use them.

The context registry owns its lock and exposes the only API for its state. A read API returns a copied record snapshot and a safe shared map reference, then releases the lock before the shim calls IMAS-Core or transforms data. The shim does not make concurrent close and read operations on the same IMAS-Core context safe; callers must keep that lifecycle valid.
