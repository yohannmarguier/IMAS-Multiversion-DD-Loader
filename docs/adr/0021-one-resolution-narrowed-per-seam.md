# One resolution, narrowed per seam

`src/conversion/path_conversion.rs` grew one resolver per operation — read,
write, delete and arraystruct context. They answer the same eleven questions
and differ in a handful of cells, so ADR 0016's and ADR 0017's policy tables
are rendered once each as a function, a resolution enum and a verdict enum.
This ADR records the shape that replaces them, and — as importantly — the
three further collapses that were considered and **deliberately not made**, so
that a future reader does not re-propose them as oversights. It is the
implementation of ADR 0016 decision 9, not a reopening of it.

Resolved by the grilling on issue #145, against the four-resolver tree at
`2703909`.

## Why one resolver and not one type

The obvious collapse is a single union resolution that each seam projects
from. It does not survive contact with the four enums as they actually stand:
they do not share a variant set. `WritePath` and `DeletePath` have no
`NoSource` and no `Unclaimed` — both fold into a refusal. `ReadPath` has no
`Unclaimed`; it folds into `NoSource(Fidelity::Unmappable)`. Only
`ContextPathResolution` carries all five.

Those folds are policy, not mechanics. ADR 0016 decision 3 is why a write with
no stored slot refuses; user story 47 is why an unclaimed read returns
not-found. A union whose consumers each receive variants they cannot produce
reintroduces exactly the dead exhaustiveness arms this work exists to delete.

So the **work** is shared and the **types** are not. One `resolve` produces one
`Resolved`. One named narrowing per seam turns it into that seam's existing
type, and each narrowing is named after, and documents, the ADR decision that
justifies its folds. Every narrowing is total over what it can receive, so no
arm exists only for exhaustiveness.

## Decisions

1. **One `resolve`, four narrowings.** `ReadPath`, `WritePath`, `DeletePath`
   and `ContextPathResolution` survive unchanged as the seams' types. The
   blast radius of their 136 references across `path_conversion.rs`,
   `seam_policy.rs` and `interpose.rs` stays zero.

2. **`Resolved` keeps `Single` and `Plan` apart, and nothing counts the
   list.** The distinction is what the rule declared — an identity, `renamed`
   or `moved` rule gives one path result; a `merged` or `split` rule gives an
   ordered candidate list. It is not a property of the list's length.

   Deciding it by length would be correct today only by accident: no `merged`
   or `split` rule in `docs/3.39.0--4.1.1.xml` declares a single source. The
   first artifact that declares one would silently flip a delete from calling
   IMAS-Core once per candidate to handing one path back to the C-facing
   layer. Per ADR 0011, a behaviour that depends on what the shipped artifact
   happens not to contain is not a decision.

   It also disarms a trap. Only `merged` and `split` rules declare a
   precedence; identity, `renamed` and `moved` declare none. The write seam
   finds its slot by `precedence == 1`, so a one-path result turned naively
   into a one-element plan would refuse every ordinary write.

3. **The read seam's two shapes become one.** `ReadPath::Translated` and
   `ReadPath::Candidates` carry the identical payload type and both production
   consumers merge them verbatim. The read loop builds its attempt list from
   either one through the same function. The distinction is dead and goes.

   The delete seam's equivalent split stays, because it is not the same
   question: `DeletePath::Translated` hands the path back for the C-facing
   layer to call IMAS-Core once, and `DeletePath::Candidates` has the seam
   policy call IMAS-Core once per candidate and synthesise its own status. A
   different party calls IMAS-Core, a different number of times. The write
   seam's split stays for ADR 0016 decision 4's unwritten-candidate list.

4. **Each seam's refusal checks are one ordered list, and every entry names
   the argument role it serves.** This is ADR 0016 decision 9's obligation
   discharged: the order a reviewer must check is one artifact in one file,
   and a fifth operation cannot inherit it by imitation.

   Decision 9 named four checks. The write seam has seven and the delete seam
   six; decision 9 omitted the DD-version stamp check (ADR 0016 decision 5)
   and the timebase value transformation check entirely. The list is the
   complete sequence, not decision 9's excerpt of it.

   The role tag is load-bearing. `resolve_write_path` serves both `field` and
   `timebase`, so a flat list cannot hold the timebase-only refusal — it would
   refuse every field carrying a legitimate COCOS sign flip — nor the
   field-only invertibility refusal.

5. **The write narrowing inverts the value transformation.** ADR 0016 decision
   7's refusal therefore happens at resolution, as decision 9 asks, instead of
   in another function in another module. `run_write` receives a
   transformation already pointing towards the stored DD version, and keeps
   all of ADR 0018's machinery: the unset-scalar sentinel skip,
   `validate_value_transformation` — which needs the caller's buffer shape and
   so cannot leave the seam — the shim-owned copy, and the unwritten-candidate
   list.

## Deliberately not done

Each of these was proposed by the review that produced issue #145, examined,
and declined. A future reader who re-proposes one should first read the reason
here.

- **`Outcome::Path` is not unified.** It carries an unenforced invariant:
  where the candidate list is non-empty, `resolved_path` and
  `value_transformation` merely restate `candidates[0]`
  (`conversion_map.rs:1187`). The case for making that a type was that four
  resolvers each match `Outcome` separately. After decision 1 above, one
  function matches it once, and the invariant is read in exactly one place.
  That does not pay for changing a public type shared with
  `src/bin/validate_equilibrium_coverage.rs`.

- **`run_read` and `run_delete` are not merged.** They are not the same loop
  with two knobs. `run_read` nests field candidates inside timebase
  candidates and applies a value transformation to each attempt; `run_delete`
  is a flat 29-line loop that does neither. Sharing them needs a policy object
  carrying four differences, one of which — visit every candidate rather than
  stop at the first that holds data — is the whole argument of ADR 0017. A
  policy field is a worse place for that argument than two loops a reader can
  see differ.

- **No context-record test seam is landed first.** The review held that the
  guard order could not be pinned by more tests because `ConversionRecord` is
  reachable only through the process-global registry. It is not:
  `ConversionRecord` derives `Clone` and every field is crate-public, so a
  test constructs one directly. The registry ceremony in the existing tests —
  a hand-picked context ID and a fictional IDS name, to dodge collisions
  through the shared map cache — is habit, not necessity.

## Consequences

- ADR 0016 decision 9 is discharged, and its own count is superseded: the
  write sequence is seven checks and the delete sequence six.
- The order is pinned by a test per adjacent pair, in both directions, built
  on a registry-free `ConversionRecord` helper. Roughly twenty such tests cost
  about three lines each rather than about thirty.
- `path_conversion.rs`'s two dead exhaustiveness arms and its production
  `unreachable!` are removed rather than documented.
- `verdict` takes two distinct wrapper types, so its two `(&str, Fidelity)`
  pairs can no longer be transposed at any of its nine call sites — the
  compile-time guarantee `seam_policy.rs`'s module doc already argues for the
  struct's fields, extended to the constructor that fills them.
- The work lands as three pull requests: the test net and the independent
  small fixes, then the check lists, then the collapse. Its only proof is a
  green suite, so the net precedes what it protects.
