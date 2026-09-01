# A loss log also reaches an unmodified HLI through an append-only file

ADR 0012 decision 7 assigned loss-log draining to an HLI before its root
context ends. That reverses here: no HLI is patched to call the exports, so
the existing channel serves no ordinary caller.

The shim writes each newly discovered non-exact entry once to a process-local,
tab-separated loss-log file. It is lazy, append-only, and has a fixed format;
the process-wide written-key set is keyed by the complete rendered line. This
answers ADR 0012's two rejected alternatives: the journal is bounded by the DD
rather than the number of operations, and the fixed column format is
machine-readable rather than an environment-gated diagnostic rendering. The
key includes occurrence identity, so its process lifetime is bounded by the
distinct loss lines encountered, rather than repeated operations on a line.

The file is an additional channel, not a change to the four exports. A root
context may end while an operation is in flight: in that case its in-memory log
is already gone and drops the entry, while the file retains it because its
written-key set has no context lifetime. That disagreement is deliberate: the
file exists to survive exactly the failure that can prevent an HLI from
draining its context log.
