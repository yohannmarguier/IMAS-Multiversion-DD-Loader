# Architecture

Visual companion to `CLAUDE.md`. The diagrams below are deliberately
**generalized**: the shim mirrors 37 IMAS-Core C exports, and drawing each one
would produce a picture nobody reads. Instead they show the *shapes* — one
layering, one type model, one skeleton every data-path seam follows, and the
three ways that skeleton ends (read, write, delete).

Per-seam behaviour stays where it is authoritative: `CLAUDE.md`'s "Where each
seam stands" table for the current policy, `docs/adr/` for why it is that way,
and the module doc comments in `src/` for the code-level contract. When a
diagram here and the code disagree, the code wins — say so in the PR that
proves it.

**Reading order.** §1–§2 are the map. §3–§6 are the type model (UML class
diagrams). §7–§10 are the runtime pipelines (sequence diagrams). §11 is the
context lifecycle.

---

## 1. System context

Where the shim sits, and what it does *not* do. The HLI is compiled against one
DD version for the life of the process; the pulse on disk was written under
another. The shim is the only thing in the picture that knows both.

```mermaid
flowchart LR
    HLI["HLI process<br/>(IMAS-Fortran, IMAS-CPP)<br/>one DD version, latched"]
    SHIM["IMAS-Multiversion-DD-Loader<br/>mirrors IMAS-Core's public C ABI"]
    CORE["IMAS-Core (libal)<br/>bound at runtime via dlopen (ADR 0001)"]
    STORE[("Pulse storage<br/>HDF5 / MDSplus / ASCII / Memory / UDA")]
    ART["Conversion-map artifact<br/>docs/3.39.0--4.1.1.xml<br/>embedded at build time (ADR 0004)"]

    HLI -->|"al_* calls, HLI-DD paths"| SHIM
    SHIM -->|"al_* calls, stored-DD paths"| CORE
    CORE --> STORE
    ART -.->|"include_str!"| SHIM
    CORE -.->|"re-entrant al_plugin_* callbacks<br/>forwarded untouched (ADR 0014)"| SHIM
    SHIM -.->|"loss log, drained by<br/>imas_mvdd_context_loss_*"| HLI

    style SHIM stroke-width:3px
```

Two asymmetries worth noting on this diagram:

- **The downward arrow is translated, the upward one mostly is not.** Values
  come back through the same buffer IMAS-Core allocated; the shim transforms
  them in place and never substitutes or frees the allocation.
- **The dotted callback arrow is the reason `reentry.rs` exists.** By the time
  IMAS-Core calls back in, the path in flight is already a *stored* path.

## 2. Layer map and the dependency rule

ADR 0015 in one picture: **only `src/interpose/` is C-facing.**
`src/conversion/` and `src/core/` never meet — policy cannot reach IMAS-Core or
process-global state, and the runtime binding makes no conversion decision.

```mermaid
flowchart TD
    subgraph SURFACE["src/lib.rs — ABI surface"]
        EXPORTS["37 mirrored al_* exports<br/>+ the shim-owned imas_mvdd_* exports"]
    end

    subgraph INTERPOSE["src/interpose/ — C-facing adaptation, one module per seam family"]
        OCC["occurrence.rs<br/>opens, discovery, registration, map cache"]
        RD["read.rs"]
        WR["write.rs"]
        DEL["delete.rs"]
        PASS["passthrough.rs<br/>untranslated + verbatim forwards"]
        LOSS["loss.rs<br/>loss-log exports"]
        SHARED["dispatch.rs · reentry.rs · refusal.rs<br/>shared machinery"]
    end

    subgraph POLICY["src/conversion/ — decisions, no global state, no IMAS-Core"]
        SEAM["seam_policy.rs<br/>run_read · run_write · run_delete<br/>decide_occurrence_registration"]
        PATHC["path_conversion.rs<br/>which stored path, at what fidelity"]
        MAP["conversion_map.rs<br/>artifact parsing + rule resolution"]
        ARTS["known_artifacts.rs"]
        OUTCOME["read_outcome.rs"]
    end

    subgraph STATE["src/registry/ + src/version/ — process state"]
        REG["context_registry.rs<br/>REGISTRY, loss logs, map cache"]
        HLIV["hli_version.rs<br/>the ADR 0005 latch"]
        STAMP["version_stamp.rs · dd_version.rs"]
    end

    subgraph BINDING["src/core/ — runtime binding only (ADR 0001)"]
        CB["core_binding.rs<br/>symbol manifest, forward_status!"]
        DL["dl.rs<br/>dlopen / dlsym"]
    end

    EXPORTS --> OCC
    EXPORTS --> RD
    EXPORTS --> WR
    EXPORTS --> DEL
    EXPORTS --> PASS
    EXPORTS --> LOSS

    OCC --> SHARED
    RD --> SHARED
    WR --> SHARED
    DEL --> SHARED

    OCC --> SEAM
    RD --> SEAM
    WR --> SEAM
    DEL --> SEAM

    OCC --> PATHC
    RD --> PATHC
    WR --> PATHC
    DEL --> PATHC

    OCC --> ARTS
    RD --> OUTCOME
    SEAM --> PATHC
    PATHC --> MAP
    ARTS --> MAP

    OCC --> REG
    RD --> REG
    WR --> REG
    DEL --> REG
    LOSS --> REG
    SHARED --> HLIV
    OCC --> STAMP
    STAMP --> OUTCOME

    SHARED --> CB
    OCC --> CB
    RD --> CB
    WR --> CB
    DEL --> CB
    PASS --> CB
    CB --> DL

    POLICY -. "forbidden: no IMAS-Core call, no REGISTRY, no OnceLock" .-> BINDING
```

The forbidden edge is not a convention — it is what makes `run_read`,
`run_write` and `run_delete` unit-testable with an injected closure standing in
for IMAS-Core.

## 3. Global class diagram

The load-bearing types, one namespace per owning module. Relationships, not
fields, are the point here; §4–§6 zoom in.

```mermaid
classDiagram
    direction LR

    namespace surface {
        class al_status_t {
            +c_int code
            +char message[256]
        }
    }

    namespace interpose {
        class CallFamily {
            <<enumeration>>
            Ordinary
            Plugin
        }
        class ReentryGuard {
            +enter() tuple
        }
    }

    namespace registry {
        class ContextRegistry {
            +record_dataentry(ctx_id)
            +record_root(...)
            +record_child(...)
            +lookup(ctx_id) ConversionRecord
            +remove(ctx_id)
            +get_or_create_map(key, load)
            +record_read_loss_at_root(...)
            +record_write_loss_at_root(...)
        }
        class ConversionRecord {
            +String resolved_path
            +ContextId pulse_ctx_id
            +ContextId root_id
            +ContextId parent_id
            +Arc map
            +Direction direction_to_stored
            +DdVersion stored_version
            +DdVersion hli_version
        }
        class LossEntry {
            +String dd_path
            +Fidelity fidelity
            +LossOperation operation
        }
        class MapCacheKey {
            +String ids
            +DdVersion stored_version
            +DdVersion hli_version
        }
    }

    namespace version {
        class DdVersion {
            <<enumeration>>
            Released
            Development
        }
        class HliVersionLatch {
            +set(version)
            +latched() DdVersion
            +conversion_is_possible() bool
        }
        class StampOutcome {
            <<enumeration>>
            Unstamped
            Stored
            Malformed
        }
    }

    namespace conversion {
        class ConversionMap {
            +load(xml) ConversionMap
            +resolve(path, direction) RuleExplanation
            +check_completeness(...)
        }
        class ArtifactMatch {
            +str xml
            +Direction direction_to_stored
        }
        class RuleExplanation {
            +String rule_id
            +Rel rel
            +MatchKind match_kind
            +SelectorStage selector_stage
            +Fidelity fidelity
            +Outcome outcome
        }
        class Resolved {
            <<enumeration>>
            Forward
            Single
            Plan
            NoSource
            Unclaimed
            Refusal
        }
        class SeamPolicy {
            +run_read(field, timebase, shape, reader) ReadVerdict
            +run_write(field, timebase, shape, source) WriteVerdict
            +run_delete(argument, delete) DeleteVerdict
            +decide_occurrence_registration(...) DiscoveryDecision
        }
    }

    namespace core {
        class CoreBinding {
            +37 resolved fn pointers
        }
        class Library {
            +open(path)
            +symbol(name)
        }
    }

    ContextRegistry "1" o-- "*" ConversionRecord : owns live contexts
    ContextRegistry "1" o-- "*" LossEntry : root-owned log
    ContextRegistry ..> MapCacheKey : caches maps by
    ConversionRecord --> ConversionMap : Arc, shared per version pair
    ConversionRecord --> DdVersion : stored + hli
    MapCacheKey --> DdVersion
    ArtifactMatch ..> ConversionMap : loaded into
    ConversionMap ..> RuleExplanation : resolve() yields
    RuleExplanation ..> Resolved : narrowed by path_conversion into
    SeamPolicy ..> Resolved : consumes
    SeamPolicy ..> al_status_t : never formats, only describes
    ReentryGuard ..> SeamPolicy : gates entry (ADR 0014)
    CallFamily ..> CoreBinding : names which half of the manifest
    CoreBinding --> Library : dlopen handle
    StampOutcome ..> SeamPolicy : discovery input
    HliVersionLatch ..> ConversionRecord : gates creation (ADR 0005)
```

## 4. The conversion-map model (`src/conversion/conversion_map.rs`)

The artifact is the specification of what conversion *is*. This is the widest
part of the type model, and the part a physicist reviewing a rule cares about.

```mermaid
classDiagram
    direction TB

    class ConversionMap {
        +Side left
        +Side right
        +Vec~Rule~ rules
        +load(xml) ConversionMap
        +resolve(path, direction) RuleExplanation
        +check_completeness(...)
    }
    class Side {
        +ArtifactDdVersion dd
        +CocosConvention cocos
    }
    class Rule {
        +String id
        +Rel rel
        +Selector left
        +Selector right
        +Vec~FromEntry~ froms
        +Fidelity fidelity_forward
        +Fidelity fidelity_reverse
    }
    class FromEntry {
        +Selector selector
        +u32 precedence
    }
    class Selector {
        <<enumeration>>
        Exact
        Subtree
        Glob
    }
    class SelectorStage {
        <<enumeration>>
        Exact
        Subtree
        Glob
    }
    class Rel {
        <<enumeration>>
        Renamed
        Moved
        Merged
        Split
        Retyped
        LeftOnly
        RightOnly
    }
    class Direction {
        <<enumeration>>
        Forward
        Reverse
    }
    class Fidelity {
        <<enumeration>>
        Exact
        PotentiallyLossy
        Lossy
        Unmappable
    }
    class ValueTransformation {
        <<enumeration>>
        None
        SignFlip
        +inverse() ValueTransformation
    }
    class Outcome {
        <<enumeration>>
        Path
        NoSource
        Refusal
    }
    class CandidatePath {
        +String path
        +u32 precedence
        +ValueTransformation value_transformation
    }
    class RefusalReason {
        <<enumeration>>
        UnservableRetype
        UnitRedefinition
        Unmappable
    }
    class RuleExplanation {
        +String rule_id
        +Rel rel
        +MatchKind match_kind
        +SelectorStage selector_stage
        +u32 precedence
        +Fidelity fidelity
        +Outcome outcome
    }

    ConversionMap "1" *-- "2" Side
    ConversionMap "1" *-- "*" Rule
    Rule "1" *-- "*" FromEntry
    Rule --> Rel
    Rule --> Fidelity : one per direction
    FromEntry --> Selector
    Selector --> SelectorStage : tried in stage order (ADR 0004)
    ConversionMap ..> Direction : resolve() argument
    ConversionMap ..> RuleExplanation : produces
    RuleExplanation *-- Outcome
    Outcome ..> CandidatePath : Path carries an ordered plan
    Outcome ..> RefusalReason : Refusal variant
    CandidatePath --> ValueTransformation
```

Three rules of this model the picture cannot show, and that are load-bearing:

- **`Retyped` always refuses**, whatever fidelity it declares — the shim cannot
  reshape an int array into an array of identifier structures.
- **Selector stages are tried Exact → Subtree → Glob**, glob only as a fallback
  where neither of the first two applies anywhere in the artifact.
  `RefusalReason::Unmappable` and the glob stage are both *unreachable* from the
  shipped artifact, and tests assert that rather than assume it (ADR 0011).
- **`Fidelity` on a rule is a claim about a read.** The write seam recomputes
  its own verdict; see §10.

## 5. Context registry and version model

Everything process-global lives here, and nothing else in the crate is allowed
to hold state.

```mermaid
classDiagram
    direction LR

    class ContextRegistry {
        +Mutex state
    }
    class State {
        +HashMap entries
        +HashMap loss_logs
        +HashMap maps
    }
    class Entry {
        <<enumeration>>
        DataEntry
        Conversion
    }
    class ConversionRecord {
        +String resolved_path
        +ContextId pulse_ctx_id
        +ContextId root_id
        +ContextId parent_id
        +Arc map
        +Direction direction_to_stored
        +DdVersion stored_version
        +DdVersion hli_version
    }
    class LossEntry {
        +String dd_path
        +Fidelity fidelity
        +LossOperation operation
    }
    class LossOperation {
        <<enumeration>>
        Read
        Write
    }
    class HliVersionLatch {
        <<OnceLock, ADR 0005>>
        Set
        Unset
        Invalid
    }
    class DdVersion {
        <<enumeration>>
        Released
        Development
    }
    class StampOutcome {
        <<enumeration>>
        Unstamped
        Stored
        Malformed
    }

    ContextRegistry *-- State
    State *-- "*" Entry
    Entry ..> ConversionRecord : Conversion variant
    Entry ..> DdVersion : DataEntry caches discovered versions by dataobjectname
    State *-- "*" LossEntry : keyed by root context id
    LossEntry --> LossOperation
    ConversionRecord --> DdVersion
    HliVersionLatch --> DdVersion
    StampOutcome ..> DdVersion : Stored variant
```

Two invariants the diagram encodes:

- **A loss log belongs to a root**, never to a cloned record — so a child
  arraystruct context contributes to the same eventual report, and a `lookup`
  snapshot can never carry a copied log.
- **The map cache holds `Weak` references**, so a `ConversionMap` lives exactly
  as long as some record references it. No eviction policy is needed or written.

## 6. One resolution, narrowed per seam (ADR 0021)

`path_conversion::resolve` answers one question — *which stored path does this
HLI argument mean, and at what fidelity* — and knows about neither seams nor
IMAS-Core. Each seam then narrows that one answer to the shape it can act on.
This is why adding a seam does not mean adding a resolver.

```mermaid
flowchart TD
    RAW["raw *const c_char argument<br/>+ live ConversionRecord"]
    RESOLVE["path_conversion::resolve()"]
    RES["Resolved<br/>Forward · Single · Plan<br/>NoSource · Unclaimed · Refusal"]

    RAW --> RESOLVE --> RES

    RES --> NCP["narrow_context_path()"]
    RES --> NRP["narrow_read_path()"]
    RES --> NWP["narrow_write_path()"]
    RES --> NDP["narrow_delete_path()"]

    NCP --> CPR["ContextPathResolution<br/>Forward · Translated · NoSource<br/>Unclaimed · Refusal"]
    NRP --> RP["ReadPath<br/>Forward · Translated(plan)<br/>NoSource · Refusal"]
    NWP --> WP["WritePath<br/>Forward · Translated<br/>Candidates · Refusal"]
    NDP --> DP["DeletePath<br/>Forward · Translated<br/>Candidates · Refusal"]

    CPR --> OCC["occurrence.rs — arraystruct open<br/>an unclaimed path refuses: the new<br/>context's stored anchor must be known"]
    RP --> RDL["seam_policy::run_read<br/>try candidates until data"]
    WP --> WRL["seam_policy::run_write<br/>write precedence 1 only"]
    DP --> DLL["seam_policy::run_delete<br/>delete every candidate"]

    style RES stroke-width:3px
```

The divergence at the bottom is deliberate and is the heart of ADR 0017: **a
write asserts a value, a delete asserts an absence.** Where a write must not fan
out, a delete must.

## 7. Opening an occurrence and discovering the stored version

Generalizes `al_begin_global_action`, `al_begin_slice_action`,
`al_begin_timerange_action` and their `al_plugin_*` twins — they differ only in
which arguments exist (`datapath` is global-action only) and which ABI symbol
`CallFamily` names.

```mermaid
sequenceDiagram
    autonumber
    participant HLI
    participant OCC as interpose::occurrence
    participant LATCH as hli_version (ADR 0005)
    participant REG as REGISTRY
    participant POL as seam_policy
    participant CORE as IMAS-Core

    HLI->>OCC: al_begin_*_action(pctx, dataobjectname, ...)
    OCC->>LATCH: latched()
    alt no HLI DD version latched
        OCC->>CORE: forward unchanged
        CORE-->>HLI: status (conversion off for this process)
    else latched
        OCC->>REG: known_stored_version(pctx, dataobjectname)
        opt cached mismatch and a datapath argument exists
            OCC->>POL: decide_datapath_translation(map, direction, path)
            POL-->>OCC: one stored spelling, or leave it alone
        end

        opt rwmode is not READ_OP (ADR 0020)
            Note over OCC,CORE: the caller's context has no reader,<br/>so ask through a shim-owned READ_OP probe
            OCC->>CORE: al_plugin_begin_global_action(READ_OP)
            OCC->>CORE: al_read_data(ids_properties/version_put/data_dictionary)
            OCC->>CORE: al_plugin_end_action(probe)
        end

        OCC->>CORE: forward the open (translated datapath if any)
        CORE-->>OCC: status, octx_id
        alt open failed
            OCC-->>HLI: status unchanged
        else opened
            OCC->>POL: decide_occurrence_registration(ids, hli, read_stamp)
            POL->>POL: classify the stamp
            alt stamp present but malformed (ADR 0009)
                POL-->>OCC: RefuseAndEnd
                OCC->>CORE: al_end_action(octx_id)
                OCC-->>HLI: refusal naming reason, path, HLI version, stored version
            else stamp absent, or equal to the HLI version (ADR 0007)
                POL-->>OCC: RegisterNothing
                OCC-->>HLI: status unchanged — passthrough from here on
            else different version, no embedded artifact
                POL-->>OCC: RegisterNothing + remember the mismatch
                OCC->>REG: remember_mismatched_occurrence(...)
                OCC-->>HLI: status unchanged
            else different version an artifact serves
                POL-->>OCC: RegisterRoot(stored, artifact)
                OCC->>REG: get_or_create_map(MapCacheKey), then record_root(...)
                OCC-->>HLI: status — this context now converts
            end
        end
    end
```

The point of the whole diagram is the branch density: **four of the five
outcomes register nothing.** Conversion is the exception, and the shim pays one
registry lookup to find that out.

## 8. The shared data-path seam skeleton

Read, write and delete pass the same gates before they diverge. Drawing the
gates once is what keeps §9 and §10 short.

```mermaid
sequenceDiagram
    autonumber
    participant HLI
    participant SEAM as interpose read/write/delete
    participant GUARD as ReentryGuard
    participant REF as refusal::live_conversion_record
    participant PC as path_conversion
    participant POL as seam_policy
    participant CORE as IMAS-Core

    HLI->>SEAM: al_read_data / al_write_data / al_delete_data
    SEAM->>GUARD: enter()
    alt already inside a shim seam on this thread (ADR 0014)
        SEAM->>CORE: forward exactly as received
        CORE-->>HLI: status
        Note right of SEAM: the path in flight is already a stored path
    else outermost call
        SEAM->>REF: live_conversion_record(ctx_id)
        REF->>REF: ADR 0005 latch first — no lock when conversion is impossible
        alt no live record (matching, unknown, unstamped, disabled)
            SEAM->>CORE: forward unchanged
            CORE-->>HLI: status
        else live conversion record
            SEAM->>PC: resolve(record, raw argument)
            PC-->>SEAM: Resolved
            SEAM->>PC: narrow_read / narrow_write / narrow_delete
            PC-->>SEAM: ReadPath / WritePath / DeletePath
            SEAM->>POL: run_read / run_write / run_delete + injected Core closure
            Note over POL,CORE: policy decides, the closure calls —<br/>this is the ADR 0015 boundary
            POL-->>SEAM: verdict
            SEAM->>SEAM: format the status, retain any loss at the root
            SEAM-->>HLI: al_status_t
        end
    end
```

Everything downstream of "live conversion record" is the only part that differs
per seam. The next two diagrams show exactly that part.

## 9. Read: try candidates until one returns data

```mermaid
sequenceDiagram
    autonumber
    participant RD as interpose::read
    participant POL as seam_policy::run_read
    participant CORE as IMAS-Core
    participant OUT as read_outcome::classify
    participant REG as REGISTRY

    RD->>POL: ReadArgument(field), ReadArgument(timebase), BufferShape, reader

    alt field refuses (unservable rule) or has no stored source
        POL-->>RD: Refusal / NotFound — no IMAS-Core call at all
    else resolvable
        loop each field candidate, in declared precedence order
            POL->>POL: validate_value_transformation(shape) before any call (ADR 0010)
            loop each timebase candidate
                POL->>CORE: al_read_data(stored field, stored timebase)
                CORE-->>POL: status + buffer
                POL->>OUT: classify(status, data pointer)
                alt Failure
                    POL-->>RD: forward IMAS-Core's own status, stop
                else Data
                    POL->>POL: apply_value_transformation in place (COCOS sign flip)
                    POL-->>RD: Data + the fidelity this candidate earned
                else NotFound
                    Note over POL: try the next candidate
                end
            end
        end
        POL-->>RD: NotFound when no candidate produced data
    end

    RD->>REG: record_read_loss_at_root(field path, fidelity) unless Exact
    RD->>REG: record_read_loss_at_root(timebase path, fidelity) unless Exact
    RD-->>RD: finish_read builds the al_status_t
```

The three-way classification (`Failure` / `NotFound` / `Data`) is ADR 0012 and
lives in exactly one function, because the ABI has two conflicting meanings for
`0` one layer down. A loss entry from a read names **your** path, not the stored
one.

## 10. Write and delete: the same plan, opposite obligations

```mermaid
sequenceDiagram
    autonumber
    participant SEAM as interpose write/delete
    participant POL as seam_policy
    participant CORE as IMAS-Core
    participant REG as REGISTRY

    alt write (ADR 0016, ADR 0018)
        SEAM->>POL: run_write(field, timebase, shape, SourceView)
        alt no stored slot, a non-primary source, or an uninvertible transformation
            POL-->>SEAM: Refusal — before IMAS-Core is called
            SEAM->>REG: record_write_loss_at_root(path, Unmappable)
            Note right of SEAM: ADR 0019 — the HLI has no refusal-tolerance<br/>branch on a put, so this tears the time slice
        else unset rank-0 scalar
            POL-->>SEAM: forward untouched — EMPTY_DOUBLE is negative,<br/>flipping it would store a fabricated measurement
        else writable
            POL->>POL: invert the declared transformation onto a shim-owned copy
            POL-->>SEAM: Forward(precedence-1 path only) + unwritten candidates
            SEAM->>CORE: al_write_data(stored path, transformed copy)
            CORE-->>SEAM: status
            opt status is 0
                SEAM->>REG: record_write_loss_at_root(each unwritten stored path,<br/>PotentiallyLossy)
            end
        end
    else delete (ADR 0017)
        SEAM->>POL: run_delete(DeleteArgument, delete closure)
        alt empty path
            POL-->>SEAM: forward — the caller's explicit whole-DATAOBJECT route
        else an escaping rule nested under the requested subtree
            POL-->>SEAM: Refusal
        else candidate plan
            loop every candidate, with no presence probe
                POL->>CORE: al_delete_data(stored candidate)
                CORE-->>POL: status — first failure retained, later candidates still attempted
            end
            POL-->>SEAM: Complete(first failure, naming the stored candidate)
        end
        Note over SEAM,REG: a delete never retains a loss entry
    end
```

The asymmetry in one line: **a write may touch only precedence 1 and must report
what it left behind; a delete must touch every candidate and reports nothing.**
See also `CLAUDE.md`'s open exposure #139 — real IMAS-Core's HDF5 `deleteData`
ignores its `path` argument, so the fan-out has no per-path effect on the only
backend that implements delete at all.

## 11. Context lifecycle

This is a state machine over **one IMAS-Core context ID**, not over a call
stack: the registry is a flat map keyed by context ID, and a pulse, its
occurrences and their arraystructs each occupy their own ID. IMAS-Core hands
those IDs out of one shared live namespace, so recording at an ID replaces
whatever used it before — which is why every state is reachable directly from
*Untracked*, and why nothing here is a nesting relation.

```mermaid
stateDiagram-v2
    [*] --> Untracked : no entry at this context id

    Untracked --> PulseEntry : al_begin_dataentry_action
    Untracked --> RootRecord : occurrence open whose stamp mismatches<br/>and has an embedded artifact
    Untracked --> ChildRecord : al_begin_arraystruct_action under a live record
    Untracked --> Untracked : occurrence open with a matching, absent<br/>or unserved stamp — nothing registered

    PulseEntry --> PulseEntry : caches each discovered occurrence version,<br/>keyed by dataobjectname
    RootRecord --> RootRecord : non-exact reads and writes append to this root's loss log
    ChildRecord --> ChildRecord : a loss query resolves this child to its root

    RootRecord --> Untracked : al_end_action on this id, on success only
    ChildRecord --> Untracked : al_end_action on this id, on success only
    PulseEntry --> Untracked : recorded over at a recycled id

    note left of PulseEntry
        No stored version and no map of its own.
        al_close_pulse is a plain forward — it does
        not remove this entry.
    end note

    note right of ChildRecord
        Inherits map, root id, pulse id and
        direction from a live parent snapshot.
        The parent does not own its lifetime.
    end note
```

Two consequences worth stating in words, because a state diagram tends to imply
the opposite:

- **Non-LIFO close is safe.** `al_end_action` removes **only its own** record,
  and only when IMAS-Core reports success. Closing a parent before its child
  leaves the child's record intact, and a refused close leaves everything alone.
- **A pulse entry outlives its pulse.** `al_close_pulse` never touches the
  registry, so a `PulseEntry` and its occurrence-version cache persist until
  something is recorded over that ID. The cache exists only to let a later
  re-open of the same occurrence translate `datapath` *before* IMAS-Core is
  called, and it is reset — not inherited — whenever a recycled ID is recorded.

---

## Where to go next

| Question | Authority |
|---|---|
| What does seam X do today? | `CLAUDE.md` → "Where each seam stands" |
| Why does it do that? | `docs/adr/0001`–`0021` |
| What is the exact contract of a function? | the module doc comment in `src/` |
| What did this look like when it was built? | `docs/history/` (frozen; paths have moved) |
| What can IMAS-Core actually do? | `docs/IMAS-CORE_FUNCTIONALITY_INVENTORY.md` |
