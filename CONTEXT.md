# IMAS-Multiversion-DD-Loader

Vocabulary for the boundary between an IMAS HLI and IMAS-Core. Fixes the terms so later prose, comments and error messages don't drift into synonyms for the same handful of concepts.

## Language

**shim**:
This project's library.
_Avoid_: loader, middleware, wrapper, translator — pick this term and use it everywhere.

**loader**:
The operating system's dynamic loader — the mechanism that loads shared libraries at runtime. Never this project, despite the repository name.
_Avoid_: using "loader" for the shim itself; if a sentence could mean either, name the OS loader explicitly.

**seam**:
An ABI entry point where the shim applies conversion policy, such as context registration or DD-path translation.
_Avoid_: conversion point, hook, interception point.

**HLI DD version**:
The DD version an HLI was built against, in which its DD paths are expressed. A compile-time constant of the calling binary for the compiled HLIs, so the shim learns it by being told, never by discovery. Being compiled is how those HLIs come to hold one version for the life of the process; holding one is the requirement, not being compiled. See `docs/adr/0005-hli-dd-version-entry-point.md`.
_Avoid_: HLI_V, caller version, source version, client version.

**self-converting HLI**:
An HLI that performs DD-version conversion itself and is therefore not a client of the shim — imas-python is the known case. The shim neither detects nor refuses one; the passthrough default means such an HLI is served by never turning the shim on.
_Avoid_: incompatible HLI, unsupported HLI, non-compiled HLI — the source language is not the criterion.

**stored DD version**:
The DD version an IDS was written under, read from `ids_properties/version_put/data_dictionary`. The other end of every conversion.
_Avoid_: backend version, target version, on-disk version — and never `getDDVersion()`, which is deliberately dead upstream.

**calling binary**:
The compiled object that calls into the shim — an executable or a shared library holding one HLI. The unit that owns an HLI DD version. One process can hold more than one, which is the case the latch guards.
_Avoid_: caller, client, consumer when the *binary* is what matters; those are fine for a person or a program in general.

**latch**:
Fixing a value on its first use for the life of the process: an identical later report is accepted, a conflicting one is refused. The rule governing the HLI DD version.
_Avoid_: lock, freeze, pin, cache, memoise.

**shim-owned export**:
A public symbol this project defines rather than mirrors from IMAS-Core, carrying the `imas_mvdd_` prefix and listed explicitly in the export-drift check. There are four — `imas_mvdd_set_hli_dd_version`, `imas_mvdd_context_loss_count`, `imas_mvdd_context_loss_at` and `imas_mvdd_context_loss_operation_at`. None of them allocates memory that crosses the boundary.
_Avoid_: extra symbol, extension, custom API, private API — they are public and supported.
**IDS name**:
The stable logical key of an IDS, such as `equilibrium`. It selects the same IDS across DD versions and is not a DD path that the shim translates.
_Avoid_: source IDS name, stored IDS name, IDS path.

**DD-version stamp**:
The optional, stored declaration of the DD version used for an IDS occurrence: `ids_properties/version_put/data_dictionary`. It is metadata about the occurrence's stored representation, not part of its scientific payload.
_Avoid_: version field, DD version, stamp — use the full term where the meaning could be unclear.

**stamped IDS occurrence**:
An IDS occurrence whose DD-version stamp is present and can therefore identify the stored DD version.

**unstamped IDS occurrence**:
An IDS occurrence whose DD-version stamp is absent. Its stored DD version is not identified by that metadata alone; handling it is a conversion-policy decision.

**conversion-map artifact**:
An XML file that states the DD conversion rules for one adjacent DD-version step. It is a supported interface for both the shim and the IMAS Data Dictionary XSLT ecosystem.

**conversion-map generator**:
The future part of the project that derives chronological DD changes for every DD version and IDS from DD information. The supplied 3.39.0 → 4.1.1 equilibrium artifact is the initial special-case artifact and uses the same XML schema and validation rules. The generated representation and pair-resolution process are not decided yet.

**rule semantics**:
The meaning of a conversion rule: whether it matches a DD path and what path or value transformation it requires. Only the shim executes rule semantics.

**rule explanation**:
Test information from the shim that identifies the rule selected for a requested DD path, its match kind, selector stage, precedence, path result, and value transformations. A `merged`/`split` rule's ambiguous direction (the side with more than one declared source) resolves to an ordered list of candidate paths — one per declared precedence, each with its own value transformation — instead of one path result, since only reading each can settle which one actually holds data (ADR 0006). The other direction still resolves to one path result and reports the matched source's own precedence.

**selector stage**:
Which of the three selector kinds a rule explanation's selector matched at: exact, subtree, or glob, tried in that order (ADR 0004). Distinct from match kind, which only says whether an explicit rule or the document-level default applied — a `Default` match kind has no selector stage. Two selectors of the same stage claiming the same path invalidates the conversion-map artifact rather than depending on XML document order.

**glob**:
A DD-path selector with wildcard characters. It is a fallback and applies only when no exact or subtree selector matches.

**path-level rule**:
A conversion rule for one requested DD path, or for a defined set of related DD paths. It can state a path change and a value transformation. A whole-IDS converter may apply these same rules while it walks an IDS.

**value transformation**:
A required change to data values during conversion, such as a COCOS sign change or a unit conversion. It is distinct from DD-path translation, is explicit and machine-readable in a path-level rule, and carries the direction it applies in — towards the stored DD version or towards the HLI's. Not every value transformation can be inverted.

**fidelity verdict**:
The conversion outcome classification retained by the shim: **exact**, **potentially lossy, unverified**, **certainly lossy**, or **unmappable**. A potentially lossy verdict describes a rule whose loss condition was not checked during the read; it does not assert that data was discarded.
_Avoid_: using "lossy" without saying whether loss is potential or certain.

**declared fidelity**:
The fidelity a conversion rule states for each direction in the conversion-map artifact. It describes what a *read* through that rule costs, so a write derives its own verdict rather than adopting it.
_Avoid_: treating it as operation-neutral, or as the fidelity verdict an operation actually earned.

**best-effort write**:
The shim's write policy: store the value where it can be placed faithfully, and refuse where it cannot, rather than store an approximation.
_Avoid_: lossy write, partial write — those name an outcome or a workflow, not the policy.

**migration write**:
A write that changes an IDS occurrence's stored DD version and rewrites its DD-version stamp. Out of scope for the shim, whose writes leave both as they are.
_Avoid_: conversion write, upgrade, in-place conversion.

**escaping rule**:
A conversion rule whose HLI-side selector falls at or under a requested DD path, but at least one of whose stored-side targets falls outside the stored subtree that path resolves to.
_Avoid_: crossing rule, leaking rule, straddling rule.

**loss log**:
The list of non-exact reads and writes recorded on a root context record: for each, the relevant complete DD path, its fidelity verdict, and which operation produced it. A read loss or refused write names the HLI-DD path as the HLI asked for it. A successful ambiguous write instead names each stored-DD candidate it deliberately left unwritten, because those are the paths where stale data may remain. It is the only channel by which loss reaches the caller when the operation *succeeded*, because a success is forced to `al_status_t.code == 0`. The HLI drains it before `al_end_action` ends the context; it does not outlive the context. See `docs/adr/0012-loss-reaches-the-caller-by-a-context-log.md`.
_Avoid_: report, journal, accumulator, diagnostics — and never call it an error channel; the reads and writes it records succeeded.

**operation** (of a loss log entry):
Which kind of call produced an entry: a read or a write. Deliberately not called a direction, because that word already names which side of a DD-version pair supplies a path.
_Avoid_: direction, mode, kind.

**read outcome**:
Which of three things one `al_read_data` call did: **failure** (`code != 0`), **not-found** (`code == 0` with a null data pointer), or **data**. One shim classifier function decides it, and nothing else in the shim compares the data pointer to null. A **scalar** (`dim == 0`) read is the one exception, and it is an exception in IMAS-Core's ABI rather than in the shim: for `dim == 0` the caller owns the buffer, so the pointer comes back unchanged and absence is signalled by IMAS-Core writing the datatype's EMPTY sentinel into it. One further classifier reads that sentinel to decide the outcome, delegating to the same function for everything else; the sentinel itself is also read by the value pipeline, which must leave a hole in an array unflipped, so one definition serves both.
_Avoid_: treating not-found as an error, or as data; passing a null `*data` for a scalar read, which IMAS-Core dereferences and so crashes on; and never confuse any of this with `Backend::readData`'s `int` convention, which sits below the C ABI and never reaches the shim.

**refusal**:
A shim-originated failure returned instead of calling IMAS-Core or instead of returning converted data, carrying the shim-owned code `IMAS_MVDD_CONVERSION_ERROR` (`-1000`). The shim reserves `-1000` to `-1099` and allocates only `-1000`; every other failure propagates IMAS-Core's own code unchanged.
_Avoid_: error, rejection, failure when the shim specifically is the origin.

**precedence**:
The explicit priority of a source path within one path-level rule. A lower number has higher priority. XML element order has no meaning, and duplicate precedence numbers are invalid.

**coverage record**:
A generated record of the DD paths that a conversion-map artifact covers. It is not hand-edited and does not define rule execution.

**completeness proof**:
`ConversionMap::check_completeness`'s result: whether every path in a real, checked-in path inventory pair is claimed by an explicit rule or a valid document-level default, and whether every rule's own primary selector corresponds to something real rather than a hallucinated path. It replaces trust in the hand-authored coverage record with an executable check; it never runs inside `resolve` and has no bearing on rule execution. See `docs/adr/0013-completeness-proven-against-real-inventories.md`.

**path inventory**:
A checked-in, real listing of the DD leaf paths for one IDS at one specific DD version. It supplies the completeness proof and, for the current embedded equilibrium artifact only, the delete seam's leaf-versus-structure safety guard; it never selects a conversion rule. See `docs/adr/0013-completeness-proven-against-real-inventories.md`.
_Avoid_: coverage record — a different, hand-authored concept the proof supersedes for verification purposes.

**conversion chain**:
An ordered in-memory representation of DD changes between two DD versions. The initial special-case artifact does not need a conversion chain. The future generator's chain and merge design is not decided yet.

**context registry**:
The single shim-owned catalogue of currently live IMAS-Core contexts whose stored DD version differs from the HLI DD version. It owns all conversion state and is accessed only through its own API; a context ID identifies at most one live registry record.
_Avoid_: context map, context stack.

**context record**:
The registry entry for one live mismatched context. It carries that context's resolved HLI DD path, conversion-map reference, pulse context ID, and optional parent context ID.

**parent context**:
The context from which an arraystruct context was opened. The shim uses the relation to construct the child record, but it does not imply lifecycle ownership or a read-time hierarchy walk.

**conversion-map cache**:
The registry-owned set of conversion maps shared by mismatched context records. A map exists only while at least one record uses it.

**seam policy**:
The shim-side rule a seam applies, separate from the binding that carries it out: which arguments translate, which contexts refuse, what fidelity a read earned, and what the shim records afterwards. A seam policy decides; it never calls IMAS-Core, reads the latch, or writes the context registry — it receives what it needs as values and returns the effects for the C-facing layer to perform (ADR 0015).
_Avoid_: policy on its own, conversion policy, business logic, rules engine, translator — and do not use it for the C-facing layer, which is the interposition, not the policy.
