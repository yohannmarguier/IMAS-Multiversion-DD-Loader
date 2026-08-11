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
The DD version an HLI was built against, in which its DD paths are expressed. A compile-time constant of the calling binary for the compiled HLIs, so the shim learns it by being told, never by discovery. See `docs/adr/0005-hli-dd-version-entry-point.md`.
_Avoid_: HLI_V, caller version, source version, client version.

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
A public symbol this project defines rather than mirrors from IMAS-Core, carrying the `imas_mvdd_` prefix and listed explicitly in the export-drift check. `imas_mvdd_set_hli_dd_version` is the first.
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
Test information from the shim that identifies the rule selected for a requested DD path, its match kind, precedence, path result, and value transformations.

**glob**:
A DD-path selector with wildcard characters. It is a fallback and applies only when no exact or subtree selector matches.

**path-level rule**:
A conversion rule for one requested DD path, or for a defined set of related DD paths. It can state a path change and a value transformation. A whole-IDS converter may apply these same rules while it walks an IDS.

**value transformation**:
A required change to data values during conversion, such as a COCOS sign change or a unit conversion. It is distinct from DD-path translation and is explicit and machine-readable in a path-level rule.

**fidelity verdict**:
The conversion outcome classification retained by the shim: **exact**, **potentially lossy, unverified**, **certainly lossy**, or **unmappable**. A potentially lossy verdict describes a rule whose loss condition was not checked during the read; it does not assert that data was discarded.
_Avoid_: using "lossy" without saying whether loss is potential or certain.

**precedence**:
The explicit priority of a source path within one path-level rule. A lower number has higher priority. XML element order has no meaning, and duplicate precedence numbers are invalid.

**coverage record**:
A generated record of the DD paths that a conversion-map artifact covers. It is not hand-edited and does not define rule execution.

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
