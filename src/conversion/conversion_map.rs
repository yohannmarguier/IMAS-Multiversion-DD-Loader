//! Conversion-map artifact loading and direction-neutral path resolution.
//!
//! See `docs/adr/0004-xml-conversion-map-artifact.md` and CONTEXT.md's
//! "conversion-map artifact", "rule explanation", "path-level rule" and
//! "glob" entries. This module parses the hand-authored equilibrium 3.39.0
//! ⇄ 4.1.1 artifact when supplied by its caller, and resolves the
//! document-level identity default and every path-level `rel` — matched
//! through any of the three selector stages
//! ADR 0004 defines (`Exact`, `Subtree`, `Glob`, tried in that order; see
//! [`ConversionMap::best_match`] and `Selector::try_match`). A resolved
//! match is an [`Outcome`]: a concrete path (with an ordered
//! [`CandidatePath`] read plan when a `merged`/`split` direction is
//! ambiguous), no source, or an explicit refusal. This keeps #48's ordered
//! candidate plans and #49's refusal/no-source outcomes at the one resolver
//! seam, before any ABI call.
//!
//! `<include>` and `<coverage>` elements are recognised and skipped: the
//! included `../common/*.xml` and `../inventory/*.txt` files are a future
//! generator concern (ADR 0004), and coverage records are generated
//! documentation that must never influence resolution (CONTEXT.md's
//! "coverage record").

use std::collections::{HashMap, HashSet};
use std::fmt;

use roxmltree::Document;

/// A released DD version naming one side of a conversion-map artifact.
///
/// Conversion-map artifacts connect released DDs; development stamps belong
/// to the HLI-facing DD-version type, not to an artifact side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactDdVersion(String);

impl ArtifactDdVersion {
    fn parse(value: &str) -> Result<Self, LoadError> {
        let mut components = value.split('.');
        let valid = (0..3).all(|_| {
            components.next().is_some_and(|component| {
                !component.is_empty()
                    && component.bytes().all(|byte| byte.is_ascii_digit())
                    && (component == "0" || !component.starts_with('0'))
            })
        }) && components.next().is_none();
        if !valid {
            return Err(LoadError::InvalidArtifactDdVersion(value.to_string()));
        }
        Ok(Self(value.to_string()))
    }
}

impl fmt::Display for ArtifactDdVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A COCOS convention identifier from a conversion-map artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CocosConvention(String);

impl CocosConvention {
    fn parse(value: &str) -> Result<Self, LoadError> {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(LoadError::InvalidCocosConvention(value.to_string()));
        }
        Ok(Self(value.to_string()))
    }
}

impl fmt::Display for CocosConvention {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One side of a conversion-map artifact: a DD version and its COCOS convention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Side {
    pub dd: ArtifactDdVersion,
    pub cocos: CocosConvention,
}

/// Which side of the map a resolution request travels from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Left DD path supplied, resolve to the right DD's spelling.
    Forward,
    /// Right DD path supplied, resolve to the left DD's spelling.
    Reverse,
}

/// Which selector stage a [`Selector`] belongs to. Per ADR 0004, exact
/// selectors are tried first, subtree selectors second, and glob selectors
/// only as a fallback when neither of the first two applies anywhere in the
/// artifact (CONTEXT.md's "glob").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SelectorStage {
    Exact,
    Subtree,
    Glob,
}

impl fmt::Display for SelectorStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SelectorStage::Exact => "exact",
            SelectorStage::Subtree => "subtree",
            SelectorStage::Glob => "glob",
        })
    }
}

/// A DD-path pattern naming one side of a rule or one `<from>` source,
/// tagged with the [`SelectorStage`] it must be tried at.
///
/// `Subtree` matches its own anchor path and every path nested under it,
/// preserving the unmatched remainder so the caller can rebuild the
/// equivalent path on the other side. `Glob` matches paths with the same
/// segment count where every non-`*` segment agrees literally; `*` stands
/// for exactly one path segment and never crosses a `/`. This grammar is
/// deliberately minimal — no `**`, no partial-segment wildcards — since no
/// rule in the approved artifact needs more and a richer grammar is easier
/// to add later than to walk back (`docs/PROTOTYPE_CRITIC.md` §1.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selector {
    Exact(String),
    Subtree(String),
    Glob(String),
}

/// One successful [`Selector`] match.
struct SelectorMatch {
    /// How specific the match was, used only to rank competing `Subtree`
    /// matches against each other (the longest anchor — i.e. the most
    /// specific selector text — wins). This is always a property of the
    /// selector's own matched text, never of the candidate path's length:
    /// `docs/PROTOTYPE_CRITIC.md` §1.4 documents a prior matcher that scored
    /// glob matches by the length of the path being converted, which let an
    /// unrelated glob rule "win" over a more specific rule purely because
    /// the input happened to be long. `Exact` and `Glob` matches never need
    /// this field compared: `ConversionMap::load` rejects any artifact where
    /// two `Exact` or two `Glob` selectors could both claim the same path.
    specificity: usize,
    /// The unmatched remainder of the path past a `Subtree` anchor —
    /// starting with `/`, or empty when the path equals the anchor itself.
    /// Always empty for `Exact` and `Glob`.
    suffix: String,
    /// The path segments a `Glob` match's `*` wildcards stood for, in
    /// pattern order — carried over to fill in the corresponding `*`
    /// wildcards on the other side's glob pattern. Always empty for `Exact`
    /// and `Subtree`.
    captures: Vec<String>,
}

impl Selector {
    fn stage(&self) -> SelectorStage {
        match self {
            Selector::Exact(_) => SelectorStage::Exact,
            Selector::Subtree(_) => SelectorStage::Subtree,
            Selector::Glob(_) => SelectorStage::Glob,
        }
    }

    fn pattern(&self) -> &str {
        match self {
            Selector::Exact(p) | Selector::Subtree(p) | Selector::Glob(p) => p,
        }
    }

    fn new(stage: SelectorStage, pattern: String) -> Self {
        match stage {
            SelectorStage::Exact => Selector::Exact(pattern),
            SelectorStage::Subtree => Selector::Subtree(pattern),
            SelectorStage::Glob => Selector::Glob(pattern),
        }
    }

    fn try_match(&self, path: &str) -> Option<SelectorMatch> {
        match self {
            Selector::Exact(pattern) => (pattern == path).then(|| SelectorMatch {
                specificity: pattern.len(),
                suffix: String::new(),
                captures: Vec::new(),
            }),
            Selector::Subtree(anchor) => {
                if path == anchor {
                    Some(SelectorMatch {
                        specificity: anchor.len(),
                        suffix: String::new(),
                        captures: Vec::new(),
                    })
                } else {
                    path.strip_prefix(anchor.as_str())
                        .filter(|rest| rest.starts_with('/'))
                        .map(|rest| SelectorMatch {
                            specificity: anchor.len(),
                            suffix: rest.to_string(),
                            captures: Vec::new(),
                        })
                }
            }
            Selector::Glob(pattern) => glob_match(pattern, path).map(|captures| SelectorMatch {
                specificity: 0,
                suffix: String::new(),
                captures,
            }),
        }
    }

    /// Renders this selector as the resolved path for a match obtained on
    /// the *other* side of the same rule: a `Subtree` or `Exact` pattern has
    /// `suffix` appended verbatim, and a `Glob` pattern has its `*`
    /// wildcards filled in positionally from `captures` (`ConversionMap::
    /// load` guarantees the counterpart selector, including every
    /// `merged`/`split` candidate, carries the same number of wildcards, so
    /// every capture is used and no `*` is left unfilled).
    fn render(&self, suffix: &str, captures: &[String]) -> String {
        match self {
            Selector::Glob(pattern) => {
                let mut captures = captures.iter();
                pattern
                    .split('/')
                    .map(|segment| {
                        if segment == "*" {
                            captures.next().map(String::as_str).unwrap_or(segment)
                        } else {
                            segment
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("/")
            }
            Selector::Exact(pattern) | Selector::Subtree(pattern) => {
                format!("{pattern}{suffix}")
            }
        }
    }
}

/// Matches `path` against `pattern` segment by segment: `*` stands for
/// exactly one path segment, every other segment must agree literally, and
/// both sides must have the same number of segments. Returns the path
/// segments each `*` stood for, in pattern order.
fn glob_match(pattern: &str, path: &str) -> Option<Vec<String>> {
    let mut pattern_segments = pattern.split('/');
    let mut path_segments = path.split('/');
    let mut captures = Vec::new();
    loop {
        match (pattern_segments.next(), path_segments.next()) {
            (Some(p), Some(s)) => {
                if p == "*" {
                    captures.push(s.to_string());
                } else if p != s {
                    return None;
                }
            }
            (None, None) => return Some(captures),
            _ => return None,
        }
    }
}

fn wildcard_count(pattern: &str) -> usize {
    pattern.split('/').filter(|segment| *segment == "*").count()
}

/// True when some concrete DD path could satisfy both glob patterns at
/// once: same segment count, and at every position at least one side is `*`
/// or the two agree literally. Used only to reject two glob selectors that
/// could both claim the same source role at load time (ADR 0004's
/// same-stage-conflict rule). This decides overlap exactly for the minimal
/// glob grammar `Selector::Glob` implements; it is not a general
/// glob-intersection prover.
fn globs_overlap(a: &str, b: &str) -> bool {
    let mut a_segments = a.split('/');
    let mut b_segments = b.split('/');
    loop {
        match (a_segments.next(), b_segments.next()) {
            (Some(x), Some(y)) => {
                if x != "*" && y != "*" && x != y {
                    return false;
                }
            }
            (None, None) => return true,
            _ => return false,
        }
    }
}

/// The fidelity a path-level rule's `<fidelity>` child states for one
/// direction. ADR 0008 further distinguishes potential from certain loss
/// within the `Lossy` value; that distinction is rule-kind-aware and is not
/// made by `ConversionMap::load`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fidelity {
    Exact,
    /// A merged or split rule that can name only one successful source after
    /// trying its candidates. The shim does not perform auxiliary reads to
    /// verify whether information was lost (ADR 0008).
    PotentiallyLossy,
    Lossy,
    Unmappable,
}

/// Which way a value transformation carries data across a DD-version boundary.
///
/// Conversion-map resolution serves reads, so its transformations always
/// describe the stored-DD value IMAS-Core returned becoming the HLI-DD value
/// the caller receives. A write must explicitly request the inverse rather
/// than assuming a transformation happens to be an involution (ADR 0016).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformationDirection {
    ToHli,
    ToStored,
}

impl TransformationDirection {
    fn inverse(self) -> Self {
        match self {
            Self::ToHli => Self::ToStored,
            Self::ToStored => Self::ToHli,
        }
    }
}

/// A required change to data values during conversion (CONTEXT.md's "value
/// transformation"), looked up by the resolved right-side path regardless of
/// which rule or default supplied that path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueTransformation {
    None,
    SignFlip {
        from_cocos: CocosConvention,
        to_cocos: CocosConvention,
        direction: TransformationDirection,
    },
}

impl ValueTransformation {
    /// The same conversion in the opposite data-flow direction, if the
    /// declared transformation can be safely reversed for a write.
    pub(crate) fn inverse(&self) -> Option<Self> {
        match self {
            Self::None => Some(Self::None),
            Self::SignFlip {
                from_cocos,
                to_cocos,
                direction,
            } if from_cocos != to_cocos => Some(Self::SignFlip {
                from_cocos: to_cocos.clone(),
                to_cocos: from_cocos.clone(),
                direction: direction.inverse(),
            }),
            // A sign flip between identical conventions is not a meaningful
            // conversion. Maps normalize it to `None`, but refusing this
            // malformed constructed value keeps a future write from guessing.
            Self::SignFlip { .. } => None,
        }
    }
}

/// One `<rule>` element's `rel` attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rel {
    Renamed,
    Merged,
    Moved,
    /// A path whose DD-side data type or rank changed. It always resolves to
    /// [`RefusalReason::UnservableRetype`], whatever fidelity it declares,
    /// because the shim cannot reshape a buffer — see
    /// `ConversionMap::refusal_before_resolution`.
    ///
    /// The artifact may also carry a `shape` attribute describing the change
    /// (the approved artifact's one retype says
    /// `shape="int_1d:struct_array"`). The loader deliberately does not read
    /// it: the refusal is unconditional, so no resolution decision could
    /// depend on it, and parsing a value nothing consumes would imply the
    /// engine acts on it. It is there for the physicist reviewing the rule.
    Retyped,
    Split,
    /// A path present only on the artifact's left side.
    ///
    /// Only its *forward* fidelity is ever consulted. `ConversionMap::load`
    /// indexes a `LeftOnly` rule into `left_sources` alone — it has no right
    /// path to index — so a reverse resolve can never select it. That is not
    /// a gap: reverse means the right side supplied the path, and this rule
    /// exists precisely because the path is absent there. A declared
    /// `reverse="unmappable"` is therefore documentation for the physicist
    /// ("this cannot be reconstructed from the other version"), never a
    /// refusal the resolver raises. `LeftOnly`'s mirror applies to
    /// [`Self::RightOnly`]. `check_completeness` is what holds the
    /// declaration honest, by proving the path really is absent from the
    /// side the rule says it is absent from.
    LeftOnly,
    /// A path present only on the artifact's right side. The mirror of
    /// [`Self::LeftOnly`]: only its *reverse* fidelity is ever consulted.
    RightOnly,
}

/// One `<from>` child of a `merged` or `split` rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FromEntry {
    selector: Selector,
    pub precedence: u32,
}

/// One path-level conversion rule. Field population depends on `rel`:
/// `Renamed`/`Moved`/`Retyped` carry both `left` and `right`; `LeftOnly`
/// carries only `left`; `RightOnly` carries only `right`; `Merged` carries
/// `right` plus left-side `froms`; `Split` carries `left` plus right-side
/// `froms`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub id: String,
    pub rel: Rel,
    left: Option<Selector>,
    right: Option<Selector>,
    pub froms: Vec<FromEntry>,
    pub fidelity_forward: Fidelity,
    pub fidelity_reverse: Fidelity,
}

/// Which rule mechanism produced a [`RuleExplanation`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchKind {
    /// An explicit `<rule>` element matched the requested path exactly.
    Explicit,
    /// No rule matched; the document-level `<default rel="identical"/>` applied.
    Default,
}

/// One candidate stored path in a `merged`/`split` rule's read plan
/// (ADR 0006), tried in ascending declared [`FromEntry::precedence`] order
/// until the first hit. Each candidate carries its own value transformation
/// because a `split` rule's multiple destinations are distinct right-side
/// paths that may be looked up independently (e.g. two separately declared
/// COCOS `<flip>` entries).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidatePath {
    pub path: String,
    pub precedence: u32,
    pub value_transformation: ValueTransformation,
}

/// The path-bearing portion of a successful resolver result before it is
/// wrapped with the selected rule's metadata and fidelity.
struct PathPlan {
    resolved_path: String,
    right_side_paths: Vec<String>,
    candidates: Vec<CandidatePath>,
}

/// Why the shim declines to convert a path at all — no IMAS-Core call is
/// possible and no translated value is returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalReason {
    UnservableRetype,
    UnitRedefinition,
    Unmappable,
}

/// What resolving a path produced beyond match information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// A concrete path plus, where necessary, its ordered read plan.
    Path {
        resolved_path: String,
        value_transformation: ValueTransformation,
        candidates: Vec<CandidatePath>,
    },
    /// No path exists on the other side for this direction.
    NoSource,
    /// The shim declines to convert this path.
    Refusal(RefusalReason),
}

/// Test information identifying the rule selected for a requested DD path,
/// its match kind, precedence, fidelity and [`Outcome`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleExplanation {
    /// The selected rule's id, or `None` for a `Default` match.
    pub rule_id: Option<String>,
    /// The selected rule's relation, or `None` for a `Default` match. Read
    /// seams use this to preserve ADR 0008's potential-versus-certain-loss
    /// distinction when exposing a fidelity verdict to the HLI.
    pub rel: Option<Rel>,
    pub match_kind: MatchKind,
    /// The selector stage that won (ADR 0004): `Some` for every `Explicit`
    /// match, `None` for a `Default` match — the document-level identity
    /// default is a fallback beyond even the glob stage, not a stage itself.
    pub selector_stage: Option<SelectorStage>,
    /// The winning source's precedence within its rule, where applicable —
    /// a `merged` rule resolved forward (one declared alias was requested)
    /// or a `split` rule resolved in reverse (one declared destination was
    /// requested). `None` whenever the resolution is unambiguous for another
    /// reason, or when it defers to a [`Self::candidates`] read plan instead
    /// because no single source can be declared the winner without reading
    /// data.
    pub precedence: Option<u32>,
    pub fidelity: Fidelity,
    pub outcome: Outcome,
}

/// Which side of a conversion-map artifact a raw DD path inventory, or a
/// rule-declared path, belongs to (issue #50's completeness proof).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventorySide {
    Left,
    Right,
}

/// One way [`ConversionMap::check_completeness`] found a path was not
/// legitimately claimed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletenessViolation {
    /// A raw inventory path resolved to neither an explicit rule nor the
    /// document-level default.
    UnclaimedInventoryPath { side: InventorySide, path: String },
    /// A raw inventory path fell through to the document-level identity
    /// default, but no path by that same spelling exists in the other
    /// side's raw inventory — the default's identity assumption is false.
    DefaultAssumesMissingCounterpart { side: InventorySide, path: String },
    /// A `left_only`/`right_only` rule declares its path gone on the other
    /// side, and that side's raw inventory lists it anyway: the artifact and
    /// the inventory contradict each other about the same DD path. `side`
    /// names the inventory that should not have contained it.
    ///
    /// [`Self::DefaultAssumesMissingCounterpart`] cannot catch this. That
    /// assertion compares the two inventories *with each other*, so a path
    /// wrongly listed on both sides satisfies it and is then claimed by the
    /// identity default and counted as supported coverage. Only comparing a
    /// rule's own declaration against the inventory catches a path the
    /// artifact says is gone while the inventory still lists it.
    SideOnlyRuleContradictedByInventory {
        rule_id: String,
        side: InventorySide,
        pattern: String,
        path: String,
    },
    /// A rule's own primary selector (`left`/`right`, never a `merged`/
    /// `split` rule's `<from>` candidates — see
    /// [`ConversionMap::check_completeness`]'s doc comment) corresponds to
    /// nothing in that side's raw inventory: a structurally invented path
    /// with no basis in the real Data Dictionary.
    RuleSelectorNotBackedByInventory {
        rule_id: String,
        side: InventorySide,
        pattern: String,
    },
}

/// A conversion-map artifact failed to load because its rule data is
/// structurally unusable — malformed XML, a missing required attribute, an
/// unrecognised enum value, or a rule shape that contradicts its own `rel`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    Xml(String),
    MissingAttribute {
        element: String,
        attribute: String,
    },
    UnknownValue {
        element: String,
        attribute: String,
        value: String,
    },
    DuplicateRuleId(String),
    /// Two rules (or `<from>` entries) register the identical literal
    /// `Exact` or `Subtree` selector for the same source role (`left`
    /// feeding a forward match, or `right` feeding a reverse match), so
    /// resolution could not pick a winner without depending on XML document
    /// order (ADR 0004).
    DuplicateSourceSelector {
        role: &'static str,
        stage: SelectorStage,
        pattern: String,
    },
    /// Two `Glob` selectors on the same source role could both match one
    /// path (ADR 0004's same-stage-conflict rule, extended to overlap
    /// rather than literal identity since `*` lets two distinct patterns
    /// compete for the same path).
    OverlappingSourceSelectors {
        role: &'static str,
        first: String,
        second: String,
    },
    OverlappingRedefineSelectors {
        first: String,
        second: String,
    },
    DuplicatePrecedence {
        rule_id: String,
        precedence: u32,
    },
    InvalidRuleShape {
        rule_id: String,
        reason: String,
    },
    DuplicateFlipPath(String),
    InvalidArtifactDdVersion(String),
    InvalidCocosConvention(String),
    MissingSide(&'static str),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Xml(msg) => write!(f, "malformed conversion-map XML: {msg}"),
            LoadError::MissingAttribute { element, attribute } => {
                write!(f, "<{element}> is missing required attribute `{attribute}`")
            }
            LoadError::UnknownValue {
                element,
                attribute,
                value,
            } => write!(
                f,
                "<{element}> attribute `{attribute}` has unrecognised value `{value}`"
            ),
            LoadError::DuplicateRuleId(id) => write!(f, "duplicate rule id `{id}`"),
            LoadError::DuplicateSourceSelector {
                role,
                stage,
                pattern,
            } => {
                write!(
                    f,
                    "duplicate {stage} source selector on the {role} side: `{pattern}`"
                )
            }
            LoadError::OverlappingSourceSelectors {
                role,
                first,
                second,
            } => write!(
                f,
                "overlapping glob source selectors on the {role} side: `{first}` and `{second}`"
            ),
            LoadError::OverlappingRedefineSelectors { first, second } => write!(
                f,
                "overlapping <redefine> glob selectors: `{first}` and `{second}`"
            ),
            LoadError::DuplicatePrecedence {
                rule_id,
                precedence,
            } => write!(
                f,
                "rule `{rule_id}` has duplicate precedence {precedence} among its <from> entries"
            ),
            LoadError::InvalidRuleShape { rule_id, reason } => {
                write!(
                    f,
                    "rule `{rule_id}` has an invalid shape for its rel: {reason}"
                )
            }
            LoadError::DuplicateFlipPath(path) => {
                write!(f, "path `{path}` appears in more than one <flip>")
            }
            LoadError::InvalidArtifactDdVersion(value) => {
                write!(f, "invalid artifact DD version `{value}`")
            }
            LoadError::InvalidCocosConvention(value) => {
                write!(f, "invalid COCOS convention `{value}`")
            }
            LoadError::MissingSide(id) => write!(f, "missing required <side id=\"{id}\"/>"),
        }
    }
}

impl std::error::Error for LoadError {}

/// One rule's contribution to a source role: a selector that must match a
/// requested path before that rule can claim it, and the index of the owning
/// rule in [`ConversionMap::rules`]. `precedence` is `Some` only when this
/// entry comes from a `merged`/`split` rule's `<from>` child (the source
/// role that carries more than one candidate) — a `Renamed`/`Moved`/
/// `Retyped`/`LeftOnly`/`RightOnly` selector, and a `merged` rule's single
/// `right` or a `split` rule's single `left`, have no declared precedence.
#[derive(Debug, Clone)]
struct SourceEntry {
    selector: Selector,
    rule_index: usize,
    precedence: Option<u32>,
}

/// The outcome of [`ConversionMap::best_match`]: which stage won, enough of
/// the match to render the other side's selector (see [`Selector::render`]),
/// which rule supplied the winning selector, and that selector's own
/// declared precedence if it came from a `<from>` entry.
struct BestMatch {
    stage: SelectorStage,
    suffix: String,
    captures: Vec<String>,
    rule_index: usize,
    precedence: Option<u32>,
}

/// Rejects an artifact where two selectors of the same [`SelectorStage`]
/// could both claim a path on this source role (ADR 0004's
/// same-stage-conflict rule): a literal duplicate for `Exact`/`Subtree`
/// (the only way two selectors at those stages can ever compete for the
/// same path — see `Selector::try_match`'s doc comment), or any overlapping
/// pair for `Glob`.
fn reject_ambiguous_sources(role: &'static str, sources: &[SourceEntry]) -> Result<(), LoadError> {
    let mut seen: HashSet<(SelectorStage, &str)> = HashSet::new();
    for entry in sources {
        let stage = entry.selector.stage();
        if stage == SelectorStage::Glob {
            continue;
        }
        let pattern = entry.selector.pattern();
        if !seen.insert((stage, pattern)) {
            return Err(LoadError::DuplicateSourceSelector {
                role,
                stage,
                pattern: pattern.to_string(),
            });
        }
    }

    let globs: Vec<&str> = sources
        .iter()
        .filter(|entry| entry.selector.stage() == SelectorStage::Glob)
        .map(|entry| entry.selector.pattern())
        .collect();
    for i in 0..globs.len() {
        for &other in &globs[(i + 1)..] {
            if globs_overlap(globs[i], other) {
                return Err(LoadError::OverlappingSourceSelectors {
                    role,
                    first: globs[i].to_string(),
                    second: other.to_string(),
                });
            }
        }
    }
    Ok(())
}

/// One `<redefine>` unit change keyed on the right-side path.
#[derive(Debug, Clone)]
struct RedefineEntry {
    selector: Selector,
    fidelity_forward: Fidelity,
    fidelity_reverse: Fidelity,
}

/// Reject unit-redefinition globs that could claim the same right-side path.
/// Their fidelity reaches the caller in the refusal explanation, so it must
/// not depend on XML document order (ADR 0004).
fn reject_ambiguous_redefines(redefines: &[RedefineEntry]) -> Result<(), LoadError> {
    for (index, redefine) in redefines.iter().enumerate() {
        for other in &redefines[(index + 1)..] {
            let first = redefine.selector.pattern();
            let second = other.selector.pattern();
            if globs_overlap(first, second) {
                return Err(LoadError::OverlappingRedefineSelectors {
                    first: first.to_string(),
                    second: second.to_string(),
                });
            }
        }
    }
    Ok(())
}

/// A loaded conversion-map artifact for one adjacent DD-version step
/// (CONTEXT.md's "conversion-map artifact").
#[derive(Debug, Clone)]
pub struct ConversionMap {
    pub ids: String,
    pub left: Side,
    pub right: Side,
    pub default_identical: bool,
    rules: Vec<Rule>,
    sign_flips: HashMap<String, (CocosConvention, CocosConvention)>,
    redefines: Vec<RedefineEntry>,
    /// Every selector that can claim a path on a forward resolve (the
    /// left-hand side of `renamed`/`moved`/`retyped`/`split`, all of
    /// `left_only`, and every `merged` rule's left-side `<from>` entries).
    left_sources: Vec<SourceEntry>,
    /// The mirror of `left_sources` for a reverse resolve.
    right_sources: Vec<SourceEntry>,
}

impl ConversionMap {
    /// Parses a conversion-map artifact from its XML text.
    ///
    /// `<include>` and `<coverage>` elements are recognised and skipped
    /// rather than resolved or validated: they name files this repository
    /// does not carry (the future conversion-map generator's concern) or are
    /// generated records that must never affect resolution.
    pub fn load(xml: &str) -> Result<Self, LoadError> {
        let doc = Document::parse(xml).map_err(|e| LoadError::Xml(e.to_string()))?;
        let root = doc.root_element();

        let ids = required_attr(&root, "ids-map", "ids")?.to_string();

        let mut left: Option<Side> = None;
        let mut right: Option<Side> = None;
        let mut default_identical = false;
        let mut rules: Vec<Rule> = Vec::new();
        let mut sign_flips: HashMap<String, (CocosConvention, CocosConvention)> = HashMap::new();
        let mut redefines: Vec<RedefineEntry> = Vec::new();
        let mut seen_rule_ids: HashSet<String> = HashSet::new();

        for child in root.children().filter(|n| n.is_element()) {
            match child.tag_name().name() {
                "side" => {
                    let id = required_attr(&child, "side", "id")?;
                    let dd = ArtifactDdVersion::parse(required_attr(&child, "side", "dd")?)?;
                    let cocos = CocosConvention::parse(required_attr(&child, "side", "cocos")?)?;
                    let side = Side { dd, cocos };
                    match id {
                        "left" => left = Some(side),
                        "right" => right = Some(side),
                        other => {
                            return Err(LoadError::UnknownValue {
                                element: "side".to_string(),
                                attribute: "id".to_string(),
                                value: other.to_string(),
                            });
                        }
                    }
                }
                "include" | "coverage" => {
                    // Deliberately not resolved/validated — see module docs.
                }
                "default" => {
                    let rel = required_attr(&child, "default", "rel")?;
                    if rel != "identical" {
                        return Err(LoadError::UnknownValue {
                            element: "default".to_string(),
                            attribute: "rel".to_string(),
                            value: rel.to_string(),
                        });
                    }
                    default_identical = true;
                }
                "rules" => {
                    for rule_node in child.children().filter(|n| n.is_element()) {
                        if rule_node.tag_name().name() != "rule" {
                            continue;
                        }
                        let rule = parse_rule(&rule_node)?;
                        if !seen_rule_ids.insert(rule.id.clone()) {
                            return Err(LoadError::DuplicateRuleId(rule.id));
                        }
                        rules.push(rule);
                    }
                }
                "transforms" => {
                    parse_transforms(&child, &mut sign_flips, &mut redefines)?;
                }
                _ => {}
            }
        }

        let left = left.ok_or(LoadError::MissingSide("left"))?;
        let right = right.ok_or(LoadError::MissingSide("right"))?;

        let mut left_sources: Vec<SourceEntry> = Vec::new();
        let mut right_sources: Vec<SourceEntry> = Vec::new();
        for (rule_index, rule) in rules.iter().enumerate() {
            match rule.rel {
                Rel::Renamed | Rel::Moved | Rel::Retyped => {
                    left_sources.push(SourceEntry {
                        selector: rule.left.clone().expect("both paths required for this rel"),
                        rule_index,
                        precedence: None,
                    });
                    right_sources.push(SourceEntry {
                        selector: rule
                            .right
                            .clone()
                            .expect("both paths required for this rel"),
                        rule_index,
                        precedence: None,
                    });
                }
                Rel::LeftOnly => {
                    left_sources.push(SourceEntry {
                        selector: rule.left.clone().expect("left_only rule has a left path"),
                        rule_index,
                        precedence: None,
                    });
                }
                Rel::RightOnly => {
                    right_sources.push(SourceEntry {
                        selector: rule
                            .right
                            .clone()
                            .expect("right_only rule has a right path"),
                        rule_index,
                        precedence: None,
                    });
                }
                Rel::Merged => {
                    for from in &rule.froms {
                        left_sources.push(SourceEntry {
                            selector: from.selector.clone(),
                            rule_index,
                            precedence: Some(from.precedence),
                        });
                    }
                    right_sources.push(SourceEntry {
                        selector: rule.right.clone().expect("merged rule has a right path"),
                        rule_index,
                        precedence: None,
                    });
                }
                Rel::Split => {
                    left_sources.push(SourceEntry {
                        selector: rule.left.clone().expect("split rule has a left path"),
                        rule_index,
                        precedence: None,
                    });
                    for from in &rule.froms {
                        right_sources.push(SourceEntry {
                            selector: from.selector.clone(),
                            rule_index,
                            precedence: Some(from.precedence),
                        });
                    }
                }
            }
        }
        reject_ambiguous_sources("left", &left_sources)?;
        reject_ambiguous_sources("right", &right_sources)?;
        reject_ambiguous_redefines(&redefines)?;

        Ok(ConversionMap {
            ids,
            left,
            right,
            default_identical,
            rules,
            sign_flips,
            redefines,
            left_sources,
            right_sources,
        })
    }

    /// Resolves `path`, supplied in the DD spelling named by `direction`'s
    /// source side, to the other side's spelling.
    ///
    /// Returns `None` when no explicit rule matches and the artifact declares
    /// no document-level identity default — a genuinely unmatched path, kept
    /// distinct from a `Default`-kind [`RuleExplanation`] so the two are never
    /// confused with each other.
    pub fn resolve(&self, path: &str, direction: Direction) -> Option<RuleExplanation> {
        if let Some(found) = self.best_match(path, direction) {
            let rule = &self.rules[found.rule_index];
            let fidelity = fidelity_for(rule, direction);

            if let Some(reason) = Self::refusal_before_resolution(rule.rel, fidelity) {
                return Some(Self::explicit_match(
                    rule,
                    &found,
                    fidelity,
                    Outcome::Refusal(reason),
                ));
            }

            return match rule.rel {
                Rel::Renamed | Rel::Moved => {
                    Some(self.resolve_single_path(rule, path, direction, &found))
                }
                Rel::Merged => Some(self.resolve_merged(rule, path, direction, &found)),
                Rel::Split => Some(self.resolve_split(rule, path, direction, &found)),
                Rel::LeftOnly | Rel::RightOnly => Some(Self::explicit_match(
                    rule,
                    &found,
                    fidelity,
                    Outcome::NoSource,
                )),
                Rel::Retyped => {
                    unreachable!("a retyped rule is always claimed by refusal_before_resolution")
                }
            };
        }

        if self.default_identical {
            return Some(self.default_path(path, direction));
        }

        None
    }

    /// The refusal a matched rule owes before any path is resolved, or
    /// `None` when resolution may proceed.
    ///
    /// The order of the two arms is load-bearing. A `retyped` rule refuses on
    /// shape whatever fidelity it declares — the shim cannot reshape an int
    /// array into an array of identifier structures, so a conversion that is
    /// lossless in principle is unavailable in practice, and the approved
    /// artifact's one retype is in fact declared `exact` in both directions.
    ///
    /// An `unmappable` fidelity then refuses for every other rule kind alike.
    /// It is a statement the artifact makes about a *direction*, not about a
    /// rule shape, so it must not be a per-resolver check that a new or
    /// edited resolver can forget — which is exactly what happened:
    /// `resolve_single_path` checked it while `resolve_merged` and
    /// `resolve_split` did not, so a `merged`/`split` rule declared
    /// `unmappable` produced a candidate read plan for the read path to
    /// execute instead of the refusal the artifact asked for.
    fn refusal_before_resolution(rel: Rel, fidelity: Fidelity) -> Option<RefusalReason> {
        match (rel, fidelity) {
            (Rel::Retyped, _) => Some(RefusalReason::UnservableRetype),
            (_, Fidelity::Unmappable) => Some(RefusalReason::Unmappable),
            _ => None,
        }
    }

    /// Resolves a `renamed` or `moved` rule's single path on the other side.
    fn resolve_single_path(
        &self,
        rule: &Rule,
        path: &str,
        direction: Direction,
        found: &BestMatch,
    ) -> RuleExplanation {
        let (target, fidelity) = match direction {
            Direction::Forward => (&rule.right, rule.fidelity_forward),
            Direction::Reverse => (&rule.left, rule.fidelity_reverse),
        };
        let target = target
            .as_ref()
            .expect("renamed or moved rule always carries both paths");
        let resolved_path = target.render(&found.suffix, &found.captures);
        let right_side_path = match direction {
            Direction::Forward => resolved_path.clone(),
            Direction::Reverse => path.to_string(),
        };
        self.explicit_path(
            rule,
            found,
            fidelity,
            direction,
            PathPlan {
                resolved_path,
                right_side_paths: vec![right_side_path],
                candidates: Vec::new(),
            },
        )
    }

    /// Resolves a `merged` rule: forward, the requested path is one declared
    /// alias and the rule's single canonical right path is the unambiguous
    /// destination; reverse, the requested path is that canonical path and
    /// every declared alias becomes an ordered candidate read plan, since
    /// only reading each can settle which one actually holds data
    /// (ADR 0006).
    fn resolve_merged(
        &self,
        rule: &Rule,
        path: &str,
        direction: Direction,
        found: &BestMatch,
    ) -> RuleExplanation {
        match direction {
            Direction::Forward => {
                let target = rule.right.as_ref().expect("merged rule has a right path");
                let resolved_path = target.render(&found.suffix, &found.captures);
                self.explicit_path(
                    rule,
                    found,
                    rule.fidelity_forward,
                    direction,
                    PathPlan {
                        right_side_paths: vec![resolved_path.clone()],
                        resolved_path,
                        candidates: Vec::new(),
                    },
                )
            }
            Direction::Reverse => {
                // The canonical path was already supplied, so it is the
                // right-side path for every candidate alike.
                let candidates =
                    self.candidate_paths(&rule.froms, found, direction, |_candidate| {
                        path.to_string()
                    });
                let resolved_path = candidates[0].path.clone();
                self.explicit_path(
                    rule,
                    found,
                    rule.fidelity_reverse,
                    direction,
                    PathPlan {
                        resolved_path,
                        right_side_paths: vec![path.to_string()],
                        candidates,
                    },
                )
            }
        }
    }

    /// Resolves a `split` rule: the dual of [`Self::resolve_merged`] with
    /// `left`/`right` swapped. Forward, the requested path is the rule's
    /// single left path and every declared destination becomes an ordered
    /// candidate read plan. Reverse, the requested path is one declared
    /// destination and the rule's single left path is the unambiguous
    /// source.
    fn resolve_split(
        &self,
        rule: &Rule,
        path: &str,
        direction: Direction,
        found: &BestMatch,
    ) -> RuleExplanation {
        match direction {
            Direction::Forward => {
                // Each candidate is itself a distinct right-side path, so
                // its value transformation must be looked up individually.
                let candidates = self.candidate_paths(&rule.froms, found, direction, |candidate| {
                    candidate.to_string()
                });
                let resolved_path = candidates[0].path.clone();
                let right_side_paths = candidates
                    .iter()
                    .map(|candidate| candidate.path.clone())
                    .collect();
                self.explicit_path(
                    rule,
                    found,
                    rule.fidelity_forward,
                    direction,
                    PathPlan {
                        resolved_path,
                        right_side_paths,
                        candidates,
                    },
                )
            }
            Direction::Reverse => {
                let target = rule.left.as_ref().expect("split rule has a left path");
                let resolved_path = target.render(&found.suffix, &found.captures);
                self.explicit_path(
                    rule,
                    found,
                    rule.fidelity_reverse,
                    direction,
                    PathPlan {
                        resolved_path,
                        right_side_paths: vec![path.to_string()],
                        candidates: Vec::new(),
                    },
                )
            }
        }
    }

    fn explicit_path(
        &self,
        rule: &Rule,
        found: &BestMatch,
        fidelity: Fidelity,
        direction: Direction,
        plan: PathPlan,
    ) -> RuleExplanation {
        if let Some(redefine_fidelity) = plan
            .right_side_paths
            .iter()
            .find_map(|path| self.redefine_for(path, direction))
        {
            return Self::explicit_match(
                rule,
                found,
                redefine_fidelity,
                Outcome::Refusal(RefusalReason::UnitRedefinition),
            );
        }
        let value_transformation =
            self.value_transformation_for(&plan.right_side_paths[0], direction);
        Self::explicit_match(
            rule,
            found,
            fidelity,
            Outcome::Path {
                resolved_path: plan.resolved_path,
                value_transformation,
                candidates: plan.candidates,
            },
        )
    }

    fn default_path(&self, path: &str, direction: Direction) -> RuleExplanation {
        let (fidelity, outcome) = match self.redefine_for(path, direction) {
            Some(fidelity) => (fidelity, Outcome::Refusal(RefusalReason::UnitRedefinition)),
            None => (
                Fidelity::Exact,
                Outcome::Path {
                    resolved_path: path.to_string(),
                    value_transformation: self.value_transformation_for(path, direction),
                    candidates: Vec::new(),
                },
            ),
        };
        RuleExplanation {
            rule_id: None,
            rel: None,
            match_kind: MatchKind::Default,
            selector_stage: None,
            precedence: None,
            fidelity,
            outcome,
        }
    }

    fn explicit_match(
        rule: &Rule,
        found: &BestMatch,
        fidelity: Fidelity,
        outcome: Outcome,
    ) -> RuleExplanation {
        RuleExplanation {
            rule_id: Some(rule.id.clone()),
            rel: Some(rule.rel),
            match_kind: MatchKind::Explicit,
            selector_stage: Some(found.stage),
            precedence: found.precedence,
            fidelity,
            outcome,
        }
    }

    fn redefine_for(&self, right_side_path: &str, direction: Direction) -> Option<Fidelity> {
        self.redefines.iter().find_map(|entry| {
            entry
                .selector
                .try_match(right_side_path)
                .map(|_| match direction {
                    Direction::Forward => entry.fidelity_forward,
                    Direction::Reverse => entry.fidelity_reverse,
                })
        })
    }

    /// Renders every `<from>` entry's selector against `found`'s match,
    /// ascending by declared precedence — the ordered read plan ADR 0006
    /// requires. `right_side_path_for` derives, from each rendered candidate
    /// path, the right-side spelling whose value transformation applies to
    /// it (the shared originally-requested path for a `merged` rule's
    /// aliases, or the candidate itself for a `split` rule's destinations).
    fn candidate_paths(
        &self,
        froms: &[FromEntry],
        found: &BestMatch,
        direction: Direction,
        right_side_path_for: impl Fn(&str) -> String,
    ) -> Vec<CandidatePath> {
        let mut froms: Vec<&FromEntry> = froms.iter().collect();
        froms.sort_by_key(|from| from.precedence);
        froms
            .into_iter()
            .map(|from| {
                let candidate_path = from.selector.render(&found.suffix, &found.captures);
                let right_side_path = right_side_path_for(&candidate_path);
                let value_transformation =
                    self.value_transformation_for(&right_side_path, direction);
                CandidatePath {
                    path: candidate_path,
                    precedence: from.precedence,
                    value_transformation,
                }
            })
            .collect()
    }

    /// The winning selector match for `path` on `direction`'s source side,
    /// across every rule regardless of whether `resolve` knows how to
    /// translate that rule's `rel` yet. Tries `Exact` sources first, then
    /// `Subtree`, then `Glob` (ADR 0004) — the first stage with any match at
    /// all wins outright, even if a later stage would also have matched.
    /// Within the `Subtree` stage the longest (most specific) anchor wins;
    /// `Exact` and `Glob` never need that tie-break because `ConversionMap::
    /// load` already rejects any artifact where two selectors of the same
    /// stage could both claim one path.
    fn best_match(&self, path: &str, direction: Direction) -> Option<BestMatch> {
        let sources = match direction {
            Direction::Forward => &self.left_sources,
            Direction::Reverse => &self.right_sources,
        };
        for stage in [
            SelectorStage::Exact,
            SelectorStage::Subtree,
            SelectorStage::Glob,
        ] {
            let winner = sources
                .iter()
                .filter(|entry| entry.selector.stage() == stage)
                .filter_map(|entry| {
                    entry
                        .selector
                        .try_match(path)
                        .map(|m| (m, entry.rule_index, entry.precedence))
                })
                .max_by_key(|(m, _, _)| m.specificity);
            if let Some((m, rule_index, precedence)) = winner {
                return Some(BestMatch {
                    stage,
                    suffix: m.suffix,
                    captures: m.captures,
                    rule_index,
                    precedence,
                });
            }
        }
        None
    }

    fn value_transformation_for(
        &self,
        right_side_path: &str,
        direction: Direction,
    ) -> ValueTransformation {
        match self.sign_flips.get(right_side_path) {
            Some((from_cocos, to_cocos)) => {
                let (from_cocos, to_cocos) = match direction {
                    // A resolved transformation is applied after IMAS-Core
                    // reads stored data, so it always points stored -> HLI.
                    Direction::Forward => (to_cocos, from_cocos),
                    Direction::Reverse => (from_cocos, to_cocos),
                };
                if from_cocos == to_cocos {
                    ValueTransformation::None
                } else {
                    ValueTransformation::SignFlip {
                        from_cocos: from_cocos.clone(),
                        to_cocos: to_cocos.clone(),
                        direction: TransformationDirection::ToHli,
                    }
                }
            }
            None => ValueTransformation::None,
        }
    }

    /// Proves this artifact's completeness against the real DD path
    /// inventories on both sides (issue #50), rather than trusting the
    /// generated `<coverage>` summary a hand-authored artifact ships with.
    /// Two things must hold:
    ///
    /// 1. Every path in `left_inventory`/`right_inventory` resolves to an
    ///    explicit rule, or to the document-level identity default *and*
    ///    the same spelling genuinely exists on the other side (a default
    ///    match whose counterpart is missing is a silent, wrong identity
    ///    assumption, not completeness).
    /// 2. Every rule's own primary selector (`left`/`right`) corresponds to
    ///    something real in its own side's raw inventory — an `Exact`
    ///    literal match, or a `Subtree`/`Glob` selector that claims at
    ///    least one real entry — catching a structurally invented path
    ///    (e.g. a typo) that would otherwise never be visited by the
    ///    inventory sweep in point 1.
    ///
    /// # What point 1 actually asserts, and what it cannot
    ///
    /// Issue #50's criterion reads "every DD path from both inventories is
    /// claimed by a rule", and that is narrower than it sounds. An artifact
    /// carrying a document-level `<default>` — which the approved one does,
    /// by design (ADR 0004, and the artifact's own header) — makes
    /// [`Self::resolve`] match *every* path, so
    /// [`CompletenessViolation::UnclaimedInventoryPath`] is unreachable for
    /// it: that violation covers a default-less artifact only. The assertion
    /// carrying the weight against a shipped-shape artifact is therefore
    /// [`CompletenessViolation::DefaultAssumesMissingCounterpart`] — a path
    /// the artifact claims by identity must genuinely exist by that same
    /// spelling on the other side, which is exactly what fails when a
    /// version drops, renames or reshapes it and no rule says so. Both
    /// reachability facts are pinned by test rather than left to a reader.
    ///
    /// The proof's scope is the two inventories, not the DD. They are the
    /// imas-dd path sets for their versions, which exclude the
    /// `ids_properties/**` and `code/**` metadata subtrees wholesale, plus
    /// `ids_properties/version_put/data_dictionary`, added by hand because
    /// the shim reads it at every open (ADR 0007) — leaving it outside the
    /// proof would leave the shim's own read path unclaimed. Nothing here
    /// proves an inventory is itself complete against its DD version;
    /// README.md states that limit where a user reads it.
    ///
    /// Two distinct tolerances keep point 2 from over-rejecting real rules
    /// — both are "paths introduced on a rule side that do not occur in the
    /// corresponding raw inventory" that the proof must include rather than
    /// reject, ADR 0013:
    ///
    /// - A `merged`/`split` rule's plural `<from>` candidates are
    ///   deliberately exempt from point 2 entirely: they are a
    ///   precedence-ordered read plan (ADR 0006) that may legitimately name
    ///   an alias not yet present in an older snapshot of the inventory —
    ///   the real approved artifact's `fold-constraints-j` rule, for
    ///   instance, lists a DD4-only canonical alias as its precedence-1 DD3
    ///   candidate, since the read plan is written to serve the whole 3.x
    ///   lineage feeding into 4.1.1, not only the pinned 3.39.0 snapshot.
    /// - A rule's own primary `Subtree` selector is backed by *either*
    ///   itself or any descendant, which is what proves a `retyped` rule's
    ///   container-level anchor complete even when a shape change (e.g.
    ///   `INT_1D` → `STRUCT_ARRAY`) means the anchor is no longer a raw DD
    ///   leaf itself and only its shape-derived child is (the real
    ///   `retype-coordinates-type` rule is this exact case) — with no
    ///   `retyped`-specific carve-out, since `left_only`/`right_only`/
    ///   `moved` subtree rules already rely on the identical tolerance.
    ///
    /// This method never mutates `self` and is never called by [`Self::
    /// resolve`] — the two are wholly independent, so a violation here has
    /// no bearing on runtime resolution (CONTEXT.md's "coverage record").
    pub fn check_completeness(
        &self,
        left_inventory: &[String],
        right_inventory: &[String],
    ) -> Result<(), Vec<CompletenessViolation>> {
        let mut violations = Vec::new();

        self.check_inventory_claimed(
            left_inventory,
            right_inventory,
            Direction::Forward,
            InventorySide::Left,
            &mut violations,
        );
        self.check_inventory_claimed(
            right_inventory,
            left_inventory,
            Direction::Reverse,
            InventorySide::Right,
            &mut violations,
        );

        for rule in &self.rules {
            if let Some(selector) = &rule.left
                && !Self::selector_backed_by_inventory(selector, left_inventory)
            {
                violations.push(CompletenessViolation::RuleSelectorNotBackedByInventory {
                    rule_id: rule.id.clone(),
                    side: InventorySide::Left,
                    pattern: selector.pattern().to_string(),
                });
            }
            if let Some(selector) = &rule.right
                && !Self::selector_backed_by_inventory(selector, right_inventory)
            {
                violations.push(CompletenessViolation::RuleSelectorNotBackedByInventory {
                    rule_id: rule.id.clone(),
                    side: InventorySide::Right,
                    pattern: selector.pattern().to_string(),
                });
            }

            // A side-only rule states an absence, and an absence is only
            // provable against the inventory that should not contain it.
            let declared_absent_from = match rule.rel {
                Rel::LeftOnly => Some((&rule.left, right_inventory, InventorySide::Right)),
                Rel::RightOnly => Some((&rule.right, left_inventory, InventorySide::Left)),
                _ => None,
            };
            if let Some((selector, inventory, side)) = declared_absent_from
                && let Some(selector) = selector
                && let Some(path) = Self::selector_first_match(selector, inventory)
            {
                violations.push(CompletenessViolation::SideOnlyRuleContradictedByInventory {
                    rule_id: rule.id.clone(),
                    side,
                    pattern: selector.pattern().to_string(),
                    path: path.clone(),
                });
            }
        }

        if violations.is_empty() {
            Ok(())
        } else {
            Err(violations)
        }
    }

    fn check_inventory_claimed(
        &self,
        inventory: &[String],
        counterpart_inventory: &[String],
        direction: Direction,
        side: InventorySide,
        violations: &mut Vec<CompletenessViolation>,
    ) {
        for path in inventory {
            match self.resolve(path, direction) {
                None => violations.push(CompletenessViolation::UnclaimedInventoryPath {
                    side,
                    path: path.clone(),
                }),
                Some(explanation) if explanation.match_kind == MatchKind::Default => {
                    if !counterpart_inventory.iter().any(|entry| entry == path) {
                        violations.push(CompletenessViolation::DefaultAssumesMissingCounterpart {
                            side,
                            path: path.clone(),
                        });
                    }
                }
                Some(_) => {}
            }
        }
    }

    /// Whether `selector` corresponds to at least one real entry in
    /// `inventory`: literal membership for `Exact`, self-or-descendant
    /// membership for `Subtree`, and at least one matching entry for `Glob`.
    fn selector_backed_by_inventory(selector: &Selector, inventory: &[String]) -> bool {
        Self::selector_first_match(selector, inventory).is_some()
    }

    /// The first `inventory` entry `selector` claims, or `None` when it
    /// claims none. One matcher serves both completeness assertions that
    /// need it, from opposite directions: a rule's own selector must match
    /// its side's inventory, and a side-only rule's selector must match
    /// nothing in the other side's. The offending entry is returned rather
    /// than a bool so the second one can name the path in its violation.
    fn selector_first_match<'a>(
        selector: &Selector,
        inventory: &'a [String],
    ) -> Option<&'a String> {
        match selector {
            Selector::Exact(pattern) => inventory.iter().find(|entry| *entry == pattern),
            Selector::Subtree(anchor) => inventory.iter().find(|entry| {
                *entry == anchor
                    || entry
                        .strip_prefix(anchor.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
            }),
            Selector::Glob(pattern) => inventory
                .iter()
                .find(|entry| glob_match(pattern, entry).is_some()),
        }
    }
}

fn fidelity_for(rule: &Rule, direction: Direction) -> Fidelity {
    match direction {
        Direction::Forward => rule.fidelity_forward,
        Direction::Reverse => rule.fidelity_reverse,
    }
}

fn required_attr<'a>(
    node: &roxmltree::Node<'a, 'a>,
    element: &str,
    attribute: &str,
) -> Result<&'a str, LoadError> {
    node.attribute(attribute)
        .ok_or_else(|| LoadError::MissingAttribute {
            element: element.to_string(),
            attribute: attribute.to_string(),
        })
}

fn parse_rel(node: &roxmltree::Node) -> Result<Rel, LoadError> {
    let value = required_attr(node, "rule", "rel")?;
    match value {
        "renamed" => Ok(Rel::Renamed),
        "merged" => Ok(Rel::Merged),
        "moved" => Ok(Rel::Moved),
        "retyped" => Ok(Rel::Retyped),
        "split" => Ok(Rel::Split),
        "left_only" => Ok(Rel::LeftOnly),
        "right_only" => Ok(Rel::RightOnly),
        other => Err(LoadError::UnknownValue {
            element: "rule".to_string(),
            attribute: "rel".to_string(),
            value: other.to_string(),
        }),
    }
}

fn parse_fidelity(
    rule_id: &str,
    node: &roxmltree::Node,
) -> Result<(Fidelity, Fidelity), LoadError> {
    let fidelity_node = node
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "fidelity")
        .ok_or_else(|| LoadError::InvalidRuleShape {
            rule_id: rule_id.to_string(),
            reason: "missing required <fidelity> child".to_string(),
        })?;
    let forward = parse_fidelity_value(&fidelity_node, "forward")?;
    let reverse = parse_fidelity_value(&fidelity_node, "reverse")?;
    Ok((forward, reverse))
}

fn parse_fidelity_value(node: &roxmltree::Node, attribute: &str) -> Result<Fidelity, LoadError> {
    let value = required_attr(node, "fidelity", attribute)?;
    match value {
        "exact" => Ok(Fidelity::Exact),
        "lossy" => Ok(Fidelity::Lossy),
        "unmappable" => Ok(Fidelity::Unmappable),
        other => Err(LoadError::UnknownValue {
            element: "fidelity".to_string(),
            attribute: attribute.to_string(),
            value: other.to_string(),
        }),
    }
}

fn parse_precedence(rule_id: &str, node: &roxmltree::Node) -> Result<u32, LoadError> {
    let raw = required_attr(node, "from", "precedence")?;
    raw.parse::<u32>().map_err(|_| LoadError::InvalidRuleShape {
        rule_id: rule_id.to_string(),
        reason: format!("<from> precedence `{raw}` is not a non-negative integer"),
    })
}

/// The [`SelectorStage`] a `<rule>` element's `subtree`/`glob` attributes
/// declare for all of its selectors (`left`, `right`, and any `<from>`
/// children) — one flag per rule, not per side, matching how the approved
/// artifact authors it (e.g. `retype-coordinates-type`'s single
/// `subtree="yes"` covers both its `left` and `right`).
fn parse_selector_stage(rule_id: &str, node: &roxmltree::Node) -> Result<SelectorStage, LoadError> {
    let subtree = node.attribute("subtree");
    let glob = node.attribute("glob");
    match (subtree, glob) {
        (Some(value), None) => {
            if value != "yes" {
                return Err(LoadError::UnknownValue {
                    element: "rule".to_string(),
                    attribute: "subtree".to_string(),
                    value: value.to_string(),
                });
            }
            Ok(SelectorStage::Subtree)
        }
        (None, Some(value)) => {
            if value != "yes" {
                return Err(LoadError::UnknownValue {
                    element: "rule".to_string(),
                    attribute: "glob".to_string(),
                    value: value.to_string(),
                });
            }
            Ok(SelectorStage::Glob)
        }
        (None, None) => Ok(SelectorStage::Exact),
        (Some(_), Some(_)) => Err(LoadError::InvalidRuleShape {
            rule_id: rule_id.to_string(),
            reason: "must not set both `subtree` and `glob`".to_string(),
        }),
    }
}

fn parse_froms(
    rule_id: &str,
    rule_node: &roxmltree::Node,
    side_attr: &str,
    stage: SelectorStage,
) -> Result<Vec<FromEntry>, LoadError> {
    let mut froms = Vec::new();
    let mut seen_precedence: HashSet<u32> = HashSet::new();
    for from_node in rule_node
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "from")
    {
        let path = required_attr(&from_node, "from", side_attr)?.to_string();
        let precedence = parse_precedence(rule_id, &from_node)?;
        if !seen_precedence.insert(precedence) {
            return Err(LoadError::DuplicatePrecedence {
                rule_id: rule_id.to_string(),
                precedence,
            });
        }
        froms.push(FromEntry {
            selector: Selector::new(stage, path),
            precedence,
        });
    }
    Ok(froms)
}

fn validate_glob_candidate_wildcards(
    rule_id: &str,
    singleton: &Selector,
    froms: &[FromEntry],
    singleton_side: &str,
    candidates_side: &str,
) -> Result<(), LoadError> {
    let singleton_wildcard_count = wildcard_count(singleton.pattern());
    if froms
        .iter()
        .any(|from| wildcard_count(from.selector.pattern()) != singleton_wildcard_count)
    {
        return Err(LoadError::InvalidRuleShape {
            rule_id: rule_id.to_string(),
            reason: format!(
                "glob `{singleton_side}` and every {candidates_side}-side <from> must carry the same number of `*` wildcards"
            ),
        });
    }
    Ok(())
}

fn parse_rule(node: &roxmltree::Node) -> Result<Rule, LoadError> {
    let id = required_attr(node, "rule", "id")?.to_string();
    let rel = parse_rel(node)?;
    let stage = parse_selector_stage(&id, node)?;
    let left = node
        .attribute("left")
        .map(|value| Selector::new(stage, value.to_string()));
    let right = node
        .attribute("right")
        .map(|value| Selector::new(stage, value.to_string()));
    let (fidelity_forward, fidelity_reverse) = parse_fidelity(&id, node)?;

    let shape_error = |reason: &str| LoadError::InvalidRuleShape {
        rule_id: id.clone(),
        reason: reason.to_string(),
    };

    let froms = match rel {
        Rel::Renamed | Rel::Moved | Rel::Retyped => {
            if left.is_none() || right.is_none() {
                return Err(shape_error("requires both `left` and `right`"));
            }
            if stage == SelectorStage::Glob {
                let left_wildcards = wildcard_count(left.as_ref().unwrap().pattern());
                let right_wildcards = wildcard_count(right.as_ref().unwrap().pattern());
                if left_wildcards != right_wildcards {
                    return Err(shape_error(
                        "glob `left` and `right` must carry the same number of `*` wildcards",
                    ));
                }
            }
            let froms = parse_froms(&id, node, "left", stage)?;
            if !froms.is_empty() {
                return Err(shape_error("must not carry <from> children"));
            }
            froms
        }
        Rel::LeftOnly => {
            if left.is_none() || right.is_some() {
                return Err(shape_error("requires `left` only"));
            }
            let froms = parse_froms(&id, node, "left", stage)?;
            if !froms.is_empty() {
                return Err(shape_error("must not carry <from> children"));
            }
            froms
        }
        Rel::RightOnly => {
            if right.is_none() || left.is_some() {
                return Err(shape_error("requires `right` only"));
            }
            let froms = parse_froms(&id, node, "right", stage)?;
            if !froms.is_empty() {
                return Err(shape_error("must not carry <from> children"));
            }
            froms
        }
        Rel::Merged => {
            if right.is_none() || left.is_some() {
                return Err(shape_error(
                    "requires `right` only, plus left-side <from> entries",
                ));
            }
            let froms = parse_froms(&id, node, "left", stage)?;
            if froms.is_empty() {
                return Err(shape_error("requires at least one <from left=\"...\"/>"));
            }
            if stage == SelectorStage::Glob {
                validate_glob_candidate_wildcards(
                    &id,
                    right.as_ref().unwrap(),
                    &froms,
                    "right",
                    "left",
                )?;
            }
            froms
        }
        Rel::Split => {
            if left.is_none() || right.is_some() {
                return Err(shape_error(
                    "requires `left` only, plus right-side <from> entries",
                ));
            }
            let froms = parse_froms(&id, node, "right", stage)?;
            if froms.is_empty() {
                return Err(shape_error("requires at least one <from right=\"...\"/>"));
            }
            if stage == SelectorStage::Glob {
                validate_glob_candidate_wildcards(
                    &id,
                    left.as_ref().unwrap(),
                    &froms,
                    "left",
                    "right",
                )?;
            }
            froms
        }
    };

    Ok(Rule {
        id,
        rel,
        left,
        right,
        froms,
        fidelity_forward,
        fidelity_reverse,
    })
}

fn parse_transforms(
    node: &roxmltree::Node,
    sign_flips: &mut HashMap<String, (CocosConvention, CocosConvention)>,
    redefines: &mut Vec<RedefineEntry>,
) -> Result<(), LoadError> {
    for child in node.children().filter(|n| n.is_element()) {
        match child.tag_name().name() {
            "cocos" => {
                let from_cocos = CocosConvention::parse(required_attr(&child, "cocos", "from")?)?;
                let to_cocos = CocosConvention::parse(required_attr(&child, "cocos", "to")?)?;
                for flip_node in child
                    .children()
                    .filter(|n| n.is_element() && n.tag_name().name() == "flip")
                {
                    let path = required_attr(&flip_node, "flip", "path")?.to_string();
                    if sign_flips
                        .insert(path.clone(), (from_cocos.clone(), to_cocos.clone()))
                        .is_some()
                    {
                        return Err(LoadError::DuplicateFlipPath(path));
                    }
                }
            }
            "redefine" => {
                let pattern = required_attr(&child, "redefine", "glob")?.to_string();
                required_attr(&child, "redefine", "left-units")?;
                required_attr(&child, "redefine", "right-units")?;
                let fidelity_node = child
                    .children()
                    .find(|n| n.is_element() && n.tag_name().name() == "fidelity")
                    .ok_or_else(|| LoadError::MissingAttribute {
                        element: "redefine".to_string(),
                        attribute: "fidelity".to_string(),
                    })?;
                let fidelity_forward = parse_fidelity_value(&fidelity_node, "forward")?;
                let fidelity_reverse = parse_fidelity_value(&fidelity_node, "reverse")?;
                redefines.push(RedefineEntry {
                    selector: Selector::new(SelectorStage::Glob, pattern),
                    fidelity_forward,
                    fidelity_reverse,
                });
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests/conversion_map.rs"]
mod tests;
