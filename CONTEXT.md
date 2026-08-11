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

**IDS name**:
The stable logical key of an IDS, such as `equilibrium`. It selects the same IDS across DD versions and is not a DD path that the shim translates.
_Avoid_: source IDS name, stored IDS name, IDS path.

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
