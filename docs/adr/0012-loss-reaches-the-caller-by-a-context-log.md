# Loss reaches the caller by a context log, not by `al_status_t`

`al_status_t` carries one `int code` and a `char message[256]`, and `code == 0` means success. A lossy read *is* a success, so the code is forced to `0` and the loss has no channel. This ADR settles where loss goes instead, which codes the shim owns, what fits in the message, and how the shim tells a not-found read from a failed one.

## A premise that did not survive

`CLAUDE.md` listed "two conflicting meanings of `0`" as the easiest bug in the project: `al_status_t.code == 0` is success while `Backend::readData` and plugin `read_data` return `0` for *not found*, with the shim translating in both directions.

The shim does not translate in either direction, because it never sees the second convention. All 37 mirrored symbols return `al_status_t`. The `int` convention belongs to `Backend::readData` (`al_backend.h:138`), a C++ virtual, and to the plugin base class — both below the C ABI. The shim cannot become a plugin either: `al_register_plugin` takes a plugin *name*, not callbacks. `CLAUDE.md` and `AGENTS.md` are corrected accordingly.

A different three-way distinction does reach the shim, and decision 3 governs it.

## Decisions

1. **A successful operation never reports loss through `al_status_t`.** On a successful read or write the code stays `0` and the message is untouched, whatever the fidelity verdict. The shim adds no cost, no accumulator and no log rendering to the read path.

   The code channel is where a *refusal* goes, and that is its primary job — but a refusal also appears in the loss log, so the two channels overlap rather than partition. This was originally stated as "the code channel carries refusals only", which the read path has never matched: the loss write happens before the outcome is dispatched, so a refused read has always earned an `UNMAPPABLE` entry, and six test sites pin it. The write path (ADR 0016) does the same, deliberately, so that the two paths agree. The redundancy is tolerated rather than removed: the entry costs nothing, and the alternative — narrowing the log write to successful outcomes — changes proven read behaviour to buy a caller nothing it can observe. Note that `UNMAPPABLE` in the log therefore means two things: a refusal, and the genuinely-unclaimed path whose read returns not-found with `code == 0`. Only the second needs the log; both use it.

2. **Each root context carries a loss log.** For every non-exact read *or write* the shim appends the relevant complete DD path, the fidelity verdict (ADR 0008), and which **operation** produced the entry. A read loss or refused write names the HLI-DD path *as the HLI asked for it*: that is the argument whose fidelity was in question. ADR 0016's one successful-but-imperfect write is different. Where one HLI path resolves to several stored candidates, the write changes only precedence 1 and records each unwritten candidate by its complete **stored-DD spelling**, because that is where another reader may still find stale data; naming the HLI path would identify the one slot now known to be correct. The operation field is deliberately not called a direction: `conversion_map::Direction` already means which side of the version pair supplies a path, and reusing the word for read-versus-write would collide with an existing term meaning something else. An operation under an arraystruct context records against the root captured in its conversion-record snapshot; a query resolves to the same root. A query on any context under one IDS therefore returns that whole IDS log, and the shim never scans for children — ADR 0003 forbids a read-time hierarchy walk and this respects it.

3. **One classifier function owns the read outcome.** `al_read_data` packs three outcomes into `al_status_t` plus the data pointer: failure (`code != 0`), not-found (`code == 0` with a null data pointer), and data. One shim function turns that pair into one of three results, and every seam consumes it — the `merged` precedence loop (ADR 0006), the value-transform gate (ADR 0010), and DD-version-stamp discovery (ADR 0007). Nothing else in the shim compares the data pointer to null. Each of those seams fails differently if the test is wrong: a miss read as a hit returns `NULL` and never tries the next alias; a null buffer reaching the transform gate is sign-flipped; an unstamped occurrence read as a failure defeats ADR 0007.

4. **The shim owns the codes `-1000` to `-1099`, and allocates only `-1000`.** IMAS-Core defines four codes — `-1`, `-2`, `-3`, `-4`, as `ERR_0 - n` with `ERR_0 = -1` (`al_defs.h:13,46-49`) — so upstream grows downward from `-1` and would need 996 further codes to reach the block. `IMAS_MVDD_CONVERSION_ERROR` (`-1000`, ADR 0010) remains the only allocated value; the rest of the block stays empty so a later effort adds a code without picking a number by hand. Every other failure propagates IMAS-Core's own code unchanged.

5. **The message drops fields in a fixed order.** The order is prefix, reason, DD path, HLI DD version, stored DD version. When the text does not fit, the two versions drop first: the caller set the HLI DD version itself and can obtain the stored one from the context. If the path alone still does not fit, it is cut **from the left** and marked with `...`, so the leaf name survives — the leaf identifies the field, and the prefix does not. The message always terminates inside `MAX_ERR_MSG_LEN`. This completes the message specification ADR 0010 left open.

6. **Exports for the log, and nothing is allocated.** `imas_mvdd_context_loss_count(ctx, *n)` and `imas_mvdd_context_loss_at(ctx, i, path_buf, buf_len, *verdict)`. No struct layout is published and no memory crosses the boundary, so ADR 0006's finding that the mirrored ABI has no free seam continues to hold. With `imas_mvdd_set_hli_dd_version` this made three shim-owned exports, all on ADR 0005's drift list.

   Decision 2's operation field is reported by the **fourth** export: `imas_mvdd_context_loss_operation_at(ctx, i, *operation)`. It is a separate accessor rather than a wider `loss_at`, because `loss_at` is already public and supported and its signature cannot change; and it is a separate accessor rather than a wider verdict enum, because an operation is not a fidelity and folding one into the other would make `IMAS_MVDD_FIDELITY_*` mean two unrelated things at once.

7. **The log dies with its root context** at `al_end_action`, alongside the context record (ADR 0003). Draining it before the context ends is the HLI's responsibility, and whether a given HLI does so is an HLI implementation concern, out of scope for this effort.

## Considered Options

- **Report loss through `al_status_t` anyway** — impossible, not merely unwise. A successful read must return `code == 0`, and there is no second out-parameter to borrow.
- **A static fidelity query keyed on an explicit version pair** — `(stored, hli, path) -> verdict`. Rejected. It is stateless and works ahead of time, but it answers only "what would this path cost", never "what did my HLI actually read", and the caller cannot name a stored DD version without a further export to tell it one. A context answers both questions by itself and removes that export.
- **A fidelity query keyed on a live context ID, one path at a time** — rejected as too fine-grained. It matches how an HLI reads, path by path, but an end user works at the IDS level and would have to know every path in advance to ask about it.
- **A process-global journal of every non-exact read** — rejected. Unbounded, and it makes process-global mutable state out of what a context already scopes.
- **A log retained per (IDS name, occurrence) after the context ends** — rejected on memory. It would let an end user ask after the HLI closed the context, but nothing bounds it: something must clear it, and every clearing rule (entry close, explicit call, next read) is a new lifetime to specify and test. Decision 7 pushes the drain to the HLI instead.
- **A shim-allocated array with a shim free function** — rejected. It would be the first shim-owned allocation crossing the boundary and the first free seam in a project that has kept ownership entirely between IMAS-Core and the HLI.
- **A published struct, filled by a count-then-fill pair** — rejected. It commits the shim to a struct layout, which is a compatibility surface a library whose job is mirroring someone else's ABI should not want. A `char *` inside such a struct would also hand out a pointer whose lifetime is the context.
- **Silence with an environment-gated diagnostic log** — rejected. Not machine-readable, and `docs/PROTOTYPE_CRITIC.md` §2.9 measured what a bespoke reporting renderer costs: `paint.rs` and `report.rs` were roughly 600 of ~1,900 non-test lines.

## Consequences

- **An unmodified HLI cannot reach the loss report.** It sees refusals through `al_status_t` and nothing else. This is consistent with ADR 0011, which already rests the defence of silent conversion on mechanism coverage rather than on notification — but the report serves a patched HLI only, and issue #38 established that patching means changing upstream repositories.
- **ADR 0008's open question closes.** The four fidelity verdicts reach the caller by the context log. They are still never verified at read time: a `merged` rule's aliases are not compared, so a read never holds two buffers, and ADR 0006's remark that detecting the disagreement is a reporting concern is settled as a deliberate refusal to detect.
- **ADR 0003's context record gains one field**, the loss log, and uses the parent context ID it already stores. The registry's existing lock covers it.
- **ADR 0010's message specification is complete**, and its `-1000` is now the allocated member of a reserved block rather than a lone number.
- **Shim-owned exports go from one to four.** The export-drift check's owned list includes the operation accessor from decision 6.
- **The read-outcome classifier is a reviewable rule, not a convention.** Spelling `*data == NULL` anywhere else in the shim is a defect a reader can see without knowing the design.
