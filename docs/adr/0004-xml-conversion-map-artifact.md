# XML conversion-map artifact and one rule interpreter

The initial `docs/3.39.0--4.1.1.xml` artifact is XML because it is a supported interface for the IMAS Data Dictionary XSLT ecosystem as well as for the shim. It contains path-level rules and explicit, machine-readable value transformations, including COCOS sign changes and unit conversions. It contains only rules that the IMAS DD can state safely; a path without such a rule is unmappable, not a manual exception.

The shim is the only interpreter of rule semantics. It matches exact selectors first, subtree selectors second, and glob selectors only as a fallback; a same-stage conflict is invalid. Precedence is explicit, unique within a rule, and independent of XML order. XML Schema and XSLT tools check structure, DD checks validate referenced information, and tests inspect the shim's rule explanation rather than reimplementing matching. Coverage records are generated, never hand-edited, and do not affect execution.

This ADR covers the special-case artifact loaded directly into memory for current tests. The future conversion-map generator's chronological-history representation and its pair-resolution or merge design remain undecided.
