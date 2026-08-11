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
An ABI entry point carrying a DD path or IDS name, hence needing translation rather than plain forwarding.
_Avoid_: conversion point, hook, interception point.

**HLI DD version**:
The DD version an HLI was built against, in which its DD paths are expressed. A compile-time constant of the calling binary for the compiled HLIs, so the shim learns it by being told, never by discovery. See `docs/adr/0002-hli-dd-version-entry-point.md`.
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
