# Critique of the DD-conversion prototype

Scope: `dd-maps/` (the conversion map format) and `middleware/` (the Rust
read-path engine that executes it), as they stand on `prototype`. Both work —
`playground/play_eq_mw_convert.f90` and `play_equilibrium.f90` pass, `cargo
test` is green, `validate.py` reports `PASS`. This document is not about
that. It's about which of the choices behind that result were load-bearing
and which were prototype-scoped conveniences that the real implementation
should not inherit without a decision. Each point below is "here's what's
fragile and why," followed by options, not a verdict — several of these are
genuinely open, and some may be fine to keep once named explicitly rather
than carried forward by default.

---

## Part 1 — The conversion map (`dd-maps/`)

### 1.1 The format's founding rationale was never exercised

`dd-maps/README.md`'s "Why XML" section justifies the format entirely in
terms of a consumer that doesn't exist yet: "if the conversion engine is
generated Fortran — a stylesheet alongside `IDSDef2F90Routines.xsl` — then a
map in XML is consumable with zero new dependencies: `document()` reads it,
and `xsl:key` indexes it for O(1) lookup." Nothing in the repo does that. The
one real consumer is `middleware/src/xml.rs`, a ~250-line hand-written XML
reader in Rust, feeding a hand-written matcher in `map.rs`. The format's
central design argument — zero new dependencies, reuse the XSLT pipeline —
was traded away the moment the engine became Rust, and nothing was revisited
to check whether XML is still the right choice for *that* consumer (a JSON
or a small DSL might have been easier to hand-parse; a real XSLT-based
engine might have made different tradeoffs than the ones the format
optimized for, like key-based O(1) lookup, which `map.rs` doesn't need
either — it memoizes with a `HashMap` instead). Worth deciding explicitly:
is a future engine still meant to be generated Fortran reading XML via
`document()`, or is Rust (or something else) the real target? The format's
grammar (globs, `**/` prefix matching, suffix matching) was designed against
the former and inherited by the latter without re-examination.

### 1.2 Direction-neutrality is asserted, not tested

The README's headline design property is that "one document ... drives both
directions." In the actual code, `middleware/src/map.rs` has no `Direction`
type, no `Reverse` variant, and its own comment says it plainly: *"the only
thing that matters throughout is `@forward` — reverse fidelity is never
consulted here."* Every `reverse` attribute in every rule, and the reverse
half of `targets()` in `validate.py`, is unexercised by any real read or
write. That's not necessarily wrong — the prototype only needed forward,
read-path conversion — but it means the bidirectionality the format was
*designed around* is currently a documentation claim, not a demonstrated
property. If the real engine needs to write DD3-shaped data back out (which
`eq_convert_3to4.f90`'s existence suggests someone anticipated), that's the
first place to spend verification effort, not the last — reverse rules for
`merged`/`split` in particular have director-dependent semantics
(`merged` reverse writes only the highest-precedence source) that have never
run.

### 1.3 Two hand-written interpreters of "what a rule means" already disagree on one thing

`validate.py`'s `Rule` class and `map.rs`'s `RuleBuilder` are two independent
implementations of the same semantics, and they already diverge on how
`precedence` is resolved for `merged`/`split`. `map.rs` explicitly sorts
`from_left`/`from_right` by the `precedence` attribute before picking a
winner (`middleware/src/map.rs:894-895`). `validate.py`'s `Rule.__init__`
just takes `<from>` elements in document order (`from_left = [f.get("left")
for f in elem.findall("from") ...]`, `dd-maps/validate.py:63-64`) and later
uses `alts[0]` as "the highest-precedence source" (`validate.py:170`) —
i.e. it trusts that whoever wrote the XML listed `<from>` in precedence
order, and never checks the `precedence` attribute at all. Today's map
happens to satisfy that (every `<from>` list in
`equilibrium/3.39.0--4.1.1.xml` is written in ascending precedence order),
so `validate.py`'s "PASS" and `map.rs`'s actual behavior agree by
convention, not by anything the XSD or the validator enforces. A map author
(or an XML formatter) reordering `<from>` elements for readability would
silently desynchronize the two — `validate.py` would keep passing while
`map.rs` picked a different source. This is small today (one map, one
author) but it's exactly the kind of drift that a second map, a second
author, or a decade of edits turns into a real incident.

**Options worth weighing:** either make `validate.py` sort by `precedence`
too (cheap, closes the gap), or stop maintaining two interpreters at all —
e.g. have the Rust engine expose the same `--explain`/`--list-rules`
introspection `validate.py` offers, and let `validate.py` shrink to grammar
+ coverage + inventory checks (a job that's genuinely different from "what
does this rule do," and worth keeping separate either way).

### 1.4 "Longest match wins" isn't one algorithm

The matcher (`Rule._match` in `validate.py`, mirrored by whatever `map.rs`'s
`Matcher` does) scores an explicit or `subtree` match by the length of the
matched anchor, but scores a `**/`-glob match by the length of the *entire
path being converted* (`validate.py:86`: `return len(path)`), not by any
property of the glob itself. Those two numbers aren't the same kind of
"specificity" — one measures how much of the rule matched, the other
measures how long the *input* happens to be. It works today because the
current 62 rules never put an explicit-subtree rule and a `**/`-glob rule in
genuine contention over a deeply-nested path, and `validate.py`'s ambiguity
check would catch a tie if one arose — but it wouldn't catch a case where
the glob rule *wins* over a more-specific-looking explicit rule simply
because the matched path was long, which is a correctness bug hiding as a
"resolved, no ambiguity" result. Worth either formalizing what "more
specific" should mean across match kinds (e.g. always compare on matched
*rule* text, never on the candidate path), or restricting `**/`-glob rules
to cases where no other rule can ever compete for the same paths (which the
`naming-3to4.xml` common rule-set already does, incidentally, by being
IDS-independent).

### 1.5 The correctness proof has no gate

`validate.py` is invoked by hand. Nothing in `CMakeLists.txt`, no CI
workflow, no pre-commit hook runs it. The README is emphatic that
`<coverage>` "is generated ... never hand-edited — a hand-typed summary
drifts from the rules it summarises and the engine would then trust a stale
promise" — but that argument applies just as much to the rules themselves
drifting from what `validate.py` last confirmed. A rule edit that breaks
coverage, introduces an overlap, or leaves a `<coverage>` block stale is
currently caught only if someone remembers to re-run the script. For a
14-file, hand-authored, physicist-reviewed artifact that's meant to be
trusted enough to drive silent data conversion, that's a thin guarantee.
This is cheap to close (wire `validate.py` into CI, or into the same CMake
step that already treats `dd-maps/*.xml` as a build dependency for
`middleware/`) and worth doing before a second map exists to also drift.

### 1.6 Composition — the format's actual scaling story — is unbuilt and unproven

The README's answer to the O(n²) file-count problem is composing
adjacent-step maps on demand, and it states a clean algebra for it:
`renamed ∘ renamed = renamed`, `scale ∘ scale = scale(product)`, `anything ∘
lossy = lossy`. It does not show — because it hasn't been tried —
what `merged ∘ split` means, how a `subtree` claim survives being
recomposed across two steps whose subtree boundaries don't align, or how a
`retyped` shape change composes with anything on either side of it. These
aren't corner cases; `merged` and `retyped` are two of the nine relationship
types the format itself defines, and the one existing map already uses both.
"One map per adjacent step, composed on demand" is currently a hypothesis
that one map pair is too small a sample to test. The risk isn't that
composition is impossible, it's that discovering *how* the algebra actually
needs to work is exactly the kind of thing that's cheap to learn now (build
a toy composer against two adjacent equilibrium maps, even a fake second
one) and expensive to discover after a real engine has been built assuming
the README's sketch was already correct.

### 1.7 The completeness proof holds against an inventory that's 12% short

This is already documented in the README as a known limitation, and it's
worth restating plainly because of what it implies for trust, not just
completeness: `validate.py`'s "every path is claimed" proof is only as good
as the inventory it checks against, and the inventory is missing
`identifier/name`, `identifier/description`, and — most importantly —
`constraints/*/sigma` and `constraints/*/source`, which are real
measurement-uncertainty data, not metadata. A conversion engine driven by
this map would silently pass those fields through the `<default
rel="identical">` fallback (probably fine, since they're not renamed) or,
worse, silently miss them entirely if a future DD version renames or
restructures them and nobody notices because they were never in the
inventory to begin with. `validate.py`'s `PASS` output doesn't distinguish
"covers the DD" from "covers what we happened to transcribe" — worth fixing
before the second map, since every map built against the current inventory
tooling inherits the same blind spot.

### 1.8 One of nine relationship types is schema-only

`retyped` is fully representable (`shape`, `key` attributes, and it's used
once, for `coordinates_type`) but `map.rs`'s own comment says the container
change it names is "reported, not performed" (`map.rs:539-542`). So today,
in practice, the format models eight relationship types end-to-end and one
as a warning label. That's a reasonable prototype cut, but it means
`retyped`'s design (a `shape` string like `"int_1d:struct_array"`, a `key`
naming the child leaf) has never been proven sufficient by an actual
implementation — it might be, or a bigger DD jump might need a richer
description (e.g. a whole subtree reshaping, not one leaf becoming one
struct field).

### 1.9 Physicist-review rules are data, not a workflow

Two rules currently carry `decision="yes"` and a `<note>` explicitly
admitting the encoded judgment might be backwards (`drop-boundary-separatrix`
says outright: "if the separatrix values are the ones that should survive,
these should become `moved` rules ... Needs a physicist"). That's honest and
good practice for a prototype. But nothing distinguishes a
`decision="yes"` rule from a `source="derived"` one at load time — the
engine applies both with equal confidence, and the only way to know a
rule's provenance is a mechanical fact vs. someone's best guess is to run
`validate.py --list-rules` and read the printout. For a map meant to convert
real reconstruction data, it may be worth a stronger boundary: surfacing
`decision="yes"` rules in the middleware's own report (they already have the
plumbing — `report.rs` prints per-rule detail), or requiring a second,
explicit sign-off attribute before such a rule is allowed to apply outside a
prototype build.

---

## Part 2 — The architecture (`middleware/`)

### 2.1 Size and scope, side by side

Roughly 3,700 lines of Rust (60-80% of it non-test, by file), plus a
purpose-built coloring and reporting subsystem, to convert one IDS, one
version pair, one direction (read), forward only. That's not a criticism of
the code quality — the discipline (no `unwrap`/`expect` outside tests, no
`eprintln!`, everything funneling through `write!`) is real and the tests
are thorough for what they cover. It's a sizing question: if this ratio of
effort-to-scope is representative, multiplying by even a handful of
(IDS × version-pair × direction) combinations is a large number, and it's
worth knowing that multiplier *before* deciding this is the shape the real
engine should take, rather than after.

### 2.2 Built read-only and forward-only, underneath a map that claims to be neither

Section 1.2 already flags that reverse-direction rules are unexercised.
Architecturally, the reason is structural, not incidental: the interception
point is `al_read_data` plus the five context-open/close calls — six
symbols, all on the read path (`al_low_level_wrap.f90`'s six redirections).
`al_write_data` is untouched by design, and a write context is registered
*only* so array-of-structure opens under it are skipped. There is no
symmetric write-side machinery, and building one isn't a small addition —
it needs the same path-reconstruction bookkeeping ctx.rs does for reads, an
inverse of every rule (`merged` reverse writes one alias and leaves the
other empty; `split` reverse writes only the primary target), and its own
answer to the `retyped` gap. If the real requirement includes writing
converted data back out — round-tripping, migrating stored pulses, feeding
a DD3-only downstream tool — the six-symbol, read-only shape chosen here
isn't a smaller version of that engine, it's a different engine that
happens to share a map format.

### 2.3 Path reconstruction shadows state it doesn't own

`ctx.rs`'s core trick — rebuilding the absolute DD path from a stack of
open/close calls (`FRAMES: Mutex<Vec<Frame>>`) — works because al-core's
context calls happen to nest in a way that lets an outside observer
reconstruct what a path *must have been*. Nothing ties that reconstruction
to al-core's actual contract for those calls beyond "this is the sequence
we've observed in practice." There's no shared type, no generated code, no
al-core-side assertion that keeps the two in sync — a change to how al-core
sequences or nests context calls (a new context kind, a reordering for an
optimization, an extra open for a feature this prototype never exercised)
would desynchronize `ctx.rs` from reality with no compiler error and no
guaranteed test failure, only a silently wrong reconstructed path feeding
the map. This is, structurally, the single biggest fragility in the
approach: it's an external shadow of internal state, verified today by one
fixture and one playground program.

**Worth weighing:** whether it's cheaper, for the real implementation, to
ask al-core for a small API addition that hands back the DD path a read
belongs to (turning this into a lookup rather than a reconstruction), versus
continuing to reconstruct it externally and accepting the coupling. The
prototype's constraint of "no al-core changes" was reasonable to get a demo
working; it may not be the right constraint for something meant to last.

### 2.4 Six hardwired symbols, not a policy

Interception lives as six explicit name substitutions in the Fortran
wrapper. Adding a seventh thing that needs conversion-awareness — a second
read entry point, a metadata call, eventually a write call per 2.2 — means
repeating the same hand-edit, in Fortran, once per symbol, rather than the
wrapper applying a general "route through the shim" policy. Fine for six;
worth a second look if the real scope grows the list.

### 2.5 One map, compiled in — scaling touches entry points, not just data

`include_str!` was a reasonable prototype shortcut (no file to locate, no
runtime I/O, cargo tracks the dependency). But it means "support a second
(IDS, version-pair)" isn't a data change, it's a code change: `include_str!`
has to become a runtime file read, `IMAS_MW_CONVERT` has to name which map
rather than being a boolean-ish switch, and — combined with 2.2 — if
different (IDS, pair, direction) combinations need different intercepted
call sets, the entry-point wiring itself may need to branch on which map is
loaded. It's worth treating "load an arbitrary map at runtime" as an early
milestone of the real implementation rather than a detail to defer, since
several other decisions (how errors surface, how the report attributes
counts per map) likely depend on it.

### 2.6 Process-global state is the price of the FFI boundary — and it's paid five times over

`FRAMES`, the `MAP` `OnceLock`, `LEDGER`, the `READS` counter, and paint's
`STATE` are all process-globals, because there's no way to thread a
request-scoped context through a C API that was never designed to carry
one. That's a real constraint, not a design misstep — but it has a
real cost that shows up in the test suite: `ctx` and `report`'s tests each
had to collapse into one function apiece specifically to dodge
cross-test interference under `cargo test`'s thread-based parallelism. That's
a tell that the global-state tax isn't free, and it will recur every time
new state needs adding (a second map's own read counter? per-map ledgers?).
Worth deciding up front whether a real implementation keeps this shape (one
mutable global per concern, more of them as scope grows) or introduces some
kind of registry/handle so state stays scoped to "the currently armed
conversion" rather than to the process.

### 2.7 Cross-checked at the value level, not the rule level

Two independent hand-written interpreters of the equilibrium map already
exist — this crate, and `eq_convert_3to4.f90` (1,132 lines of hand-transcribed
Fortran). They're both checked against the same oracle
(`imas-python-fixtures`'s DD4.1.1 half), which is good practice, but it's an
integration check on *one populated example*, not a check that the two
engines agree on the map's *rules* under the cases that actually stress a
merge/split/redefine: an alias populated but its modern name absent (forcing
the fallback), both aliases populated with conflicting values (the case
`fold-constraints-j`'s own fidelity note calls "lossy" precisely because it
can happen), a `chi_squared` variance-redefinition path with no value at
all. A rule change could pass both engines' fixture-level tests while the
two still silently disagree on inputs the fixture doesn't cover.

### 2.8 A concrete anomaly worth chasing before scaling

The checked-in `playground/play_equilibrium_report.md` shows
`constraints/j_phi`, `ggd/j_phi` and `ggd/b_field_phi` — all merge targets
whose left-hand anchor is itself an array-of-structure — coming back
**empty** on the converted side, while `profiles_1d/j_phi`,
`profiles_2d/j_phi` and `profiles_2d/b_field_phi` — the same kind of merge,
but on plain array leaves, not array-of-structure anchors — convert
correctly and match the native reading exactly. That pattern lines up with
the *other* documented AoS complication in this codebase: renames of an
array-of-structure "have to happen at `al_begin_arraystruct_action`, not at
the read... That was a real segfault in `play_equilibrium.f90`." If the
merge fallback for an AoS-anchored `merged` rule has the same gap and simply
degrades to "empty" instead of segfaulting, that's a silent correctness bug
sitting a few keystrokes away from being read as "expected ABSENT" and
shipped. It's flagged here as an observation to verify (with tracing on and
a debugger, per the CLAUDE.md recipe), not a confirmed root cause — but it's
exactly the kind of thing worth resolving before this shape of interception
is trusted for more merges.

### 2.9 A third of the crate is observability

`paint.rs` + `report.rs` are roughly 600 of ~1,900 non-test lines — more
investment in explaining the conversion than in a lot of the conversion
logic itself. That's defensible for a prototype whose job is partly to be
legible to a reviewer (a warning channel nobody reads is not one, as the
architecture notes put it), but it's infrastructure with its own
maintenance surface (the padding-through-ANSI bug already found and fixed
is a preview of the class of bug this kind of code invites) and its own
discipline every future capability has to remember to honor (route through
`write!`, never `eprintln!`, respect `IMAS_MW_COLOR`). Worth an explicit
call on whether the real system keeps a bespoke terminal report at this
level of investment, or moves richer reporting to something structured
(the crate already exports counts via `imas_mw_conversion_report()` /
`imas_mw_conversion_losses()` for exactly this reason) and lets a caller
choose how to render it.

### 2.10 Toolchain coupling accepted for a narrow capability

Cargo becomes an unconditional, non-optional build dependency of
`al-fortran` — not gated behind a CMake option — and the cluster note about
matching `GCCcore` generations between Rust and gfortran is a real
operational constraint. That trade might be entirely worth it for what the
real system ends up doing, but it's worth re-examining now that the
capability it's paying for is narrower than "the real thing": one IDS, one
direction, one version pair. If the real target is meant to run on every
platform `al-fortran` already supports without a matched Rust toolchain,
this dependency — accepted here to get a demo working with `AL_DEVELOPMENT_LAYOUT`-style
speed — may need to become optional (behind the macro/`.F90`-preprocessing
change the notes already say was deliberately deferred) rather than
hardwired.

---

## Open questions worth settling before writing the real thing

These aren't recommendations — they're the decisions this prototype
deferred, made visible so the real implementation makes them on purpose
rather than by inheriting the prototype's defaults:

1. **Read-only, or read+write?** Everything in §2.2 and §1.2 follows from
   this. If write-path conversion is in scope, the interception point
   probably needs to change shape, not just grow.
2. **One version-pair at a time, or several loaded together?** Determines
   whether `include_str!` + a boolean env var is a shortcut worth keeping a
   while longer or the first thing to replace.
3. **Per-call interception, or whole-entry conversion?** A batch engine
   operating over an in-memory DD-typed tree might make `retyped` (and any
   bigger reshape a larger version jump needs) tractable in a way a
   per-`al_read_data` shim structurally can't be.
4. **How many independent implementations of "what a rule means" is the
   real system willing to maintain?** Today there are at least three
   (the XSD's grammar, `validate.py`'s Python semantics, `map.rs`'s Rust
   semantics), already diverging on one attribute. That number should be a
   choice, not an accumulation.
5. **Does composition actually work?** Cheapest to find out with a toy
   second map now, before a real second (IDS, pair) is built assuming the
   README's algebra sketch was load-bearing.
