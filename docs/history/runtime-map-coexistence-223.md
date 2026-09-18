# Runtime-map coexistence plans — #223

The graph-backed conversion-map constructor now turns one dated predecessor
declaration plus its corroborating successor edge into a candidate plan only
when the requested DD endpoints prove an ordered coexistence: one endpoint
has one spelling and the other has both. The successor is precedence one;
query row order and name similarity play no part. A non-coexisting endpoint
continues to receive the established one-to-one rename rule.

Each candidate is checked against exact endpoint metadata before the plan is
published. A representation that cannot be served as a path-only conversion
does not become a candidate. The existing `ConversionMap` resolver and ABI
seam policies execute the plan unchanged.

The controlled graph ABI suite covers `j_tor` / `j_phi`, with a second
`b_field_tor` / `b_field_phi` pair in the same scope. It verifies a 4.1.1 HLI
against a 3.42.0 occurrence reads the successor first and falls back to the
predecessor, writes only the successor while recording the skipped stored
path, and deletes both candidates in order without a presence read. The
reverse 3.42.0-to-4.1.1 write refuses the non-primary shared source.

Verification run: `cargo test --lib`; a stub-only Debug CMake build; and its
full `ctest` registration (256 tests). Live Neo4j and real-IMAS-Core cases
remain covered by their existing conditional CI environments; this change
does not alter graph acquisition deadlines, arraystruct opening (#224), or
the interpreter's value-transformation mechanisms.
