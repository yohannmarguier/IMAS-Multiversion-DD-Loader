//! Conversion-map artifact loading and direction-neutral path resolution.
//!
//! See `docs/adr/0004-xml-conversion-map-artifact.md` and CONTEXT.md's
//! "conversion-map artifact", "rule explanation", "path-level rule", "glob"
//! and "refusal" entries. This module parses the hand-authored equilibrium
//! 3.39.0 ⇄ 4.1.1 artifact when supplied by its caller, and resolves the
//! document-level identity default plus every path-level `rel` except
//! `merged`, `moved` and `split` — matched through any of the three selector
//! stages ADR 0004 defines (`Exact`, `Subtree`, `Glob`, tried in that order;
//! see [`ConversionMap::best_match`] and `Selector::try_match`). A resolved
//! match is one of three [`Outcome`]s, not always a translated path:
//! `renamed` and the identity default translate to a path (refusing instead
//! when their declared fidelity is `unmappable`, or when the resolved
//! right-side path falls under a `<redefine>` unit change — ADR 0006, ADR
//! 0010); `retyped` always refuses, since this project's single-buffer
//! read/write pipeline cannot serve a container/type change (ADR 0006);
//! `left_only`/`right_only` resolve to no source in their one matchable
//! direction, or to a refusal when the artifact declares that direction
//! unmappable. `merged`, `moved` and `split` still only parse structurally
//! and participate in selector matching — so a path any of them claims is
//! never misreported as an unmatched, defaulted-to-identity path — but
//! `resolve` does not yet turn a match on one of them into an outcome; a
//! later issue extends that (#48).
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
    /// load` guarantees both sides of a glob rule carry the same number of
    /// wildcards, so every capture is used and no `*` is left unfilled).
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
    Lossy,
    Unmappable,
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
    },
}

/// One `<rule>` element's `rel` attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rel {
    Renamed,
    Merged,
    Moved,
    Retyped,
    Split,
    LeftOnly,
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

/// Why the shim declines to convert a path at all — no IMAS-Core call is
/// possible and no translated value is returned (CONTEXT.md's "refusal").
/// Distinct from [`Fidelity`]: fidelity states what the artifact says about
/// data preservation, refusal states that the shim will not attempt the
/// conversion regardless of what that fidelity is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalReason {
    /// A `retyped` rule whose container/type change this project's
    /// single-buffer read/write value pipeline cannot serve — e.g. an
    /// `INT_1D` becoming a `STRUCT_ARRAY` needs a fabricated arraystruct
    /// context, not a value transformation (ADR 0006).
    UnservableRetype,
    /// The resolved path falls under a `<redefine>` unit change with no
    /// safe numeric transform (ADR 0006, ADR 0010).
    UnitRedefinition,
    /// The matching rule or default declares this direction `unmappable`.
    Unmappable,
}

/// What resolving a path produced, beyond the match information every
/// [`RuleExplanation`] carries — CONTEXT.md's rule explanation "path
/// result", which is not always a translated path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// A concrete DD path to query or write on the other side.
    Path {
        resolved_path: String,
        value_transformation: ValueTransformation,
    },
    /// No path exists on the other side for this direction: legitimately
    /// absent, not an error (a `left_only`/`right_only` relation resolved in
    /// its declared-lossy direction).
    NoSource,
    /// The shim declines to convert this path at all.
    Refusal(RefusalReason),
}

/// Test information identifying the rule selected for a requested DD path:
/// its match kind, precedence, fidelity, and path result — the latter an
/// [`Outcome`], since a path result is not always a translated path
/// (CONTEXT.md's "rule explanation").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleExplanation {
    /// The selected rule's id, or `None` for a `Default` match.
    pub rule_id: Option<String>,
    pub match_kind: MatchKind,
    /// The selector stage that won (ADR 0004): `Some` for every `Explicit`
    /// match, `None` for a `Default` match — the document-level identity
    /// default is a fallback beyond even the glob stage, not a stage itself.
    pub selector_stage: Option<SelectorStage>,
    /// The winning source's precedence within its rule, where applicable
    /// (a `merged`/`split` `<from>` entry). Always `None` for the match
    /// kinds this issue resolves.
    pub precedence: Option<u32>,
    pub fidelity: Fidelity,
    pub outcome: Outcome,
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
/// rule in [`ConversionMap::rules`].
#[derive(Debug, Clone)]
struct SourceEntry {
    selector: Selector,
    rule_index: usize,
}

/// The outcome of [`ConversionMap::best_match`]: which stage won, enough of
/// the match to render the other side's selector (see [`Selector::render`]),
/// and which rule supplied the winning selector.
struct BestMatch {
    stage: SelectorStage,
    suffix: String,
    captures: Vec<String>,
    rule_index: usize,
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

/// One `<redefine>` unit change from a conversion-map artifact's
/// `<transforms>` section. Keyed on the right-side path so it never
/// competes with a structural rule for ownership of a path (the artifact's
/// own framing); consulted only after a right-side path is already known,
/// the same way `sign_flips` is.
#[derive(Debug, Clone)]
struct RedefineEntry {
    selector: Selector,
    fidelity_forward: Fidelity,
    fidelity_reverse: Fidelity,
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
                    });
                    right_sources.push(SourceEntry {
                        selector: rule
                            .right
                            .clone()
                            .expect("both paths required for this rel"),
                        rule_index,
                    });
                }
                Rel::LeftOnly => {
                    left_sources.push(SourceEntry {
                        selector: rule.left.clone().expect("left_only rule has a left path"),
                        rule_index,
                    });
                }
                Rel::RightOnly => {
                    right_sources.push(SourceEntry {
                        selector: rule
                            .right
                            .clone()
                            .expect("right_only rule has a right path"),
                        rule_index,
                    });
                }
                Rel::Merged => {
                    for from in &rule.froms {
                        left_sources.push(SourceEntry {
                            selector: from.selector.clone(),
                            rule_index,
                        });
                    }
                    right_sources.push(SourceEntry {
                        selector: rule.right.clone().expect("merged rule has a right path"),
                        rule_index,
                    });
                }
                Rel::Split => {
                    left_sources.push(SourceEntry {
                        selector: rule.left.clone().expect("split rule has a left path"),
                        rule_index,
                    });
                    for from in &rule.froms {
                        right_sources.push(SourceEntry {
                            selector: from.selector.clone(),
                            rule_index,
                        });
                    }
                }
            }
        }
        reject_ambiguous_sources("left", &left_sources)?;
        reject_ambiguous_sources("right", &right_sources)?;

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

            // A rule of a kind this issue does not yet resolve (`merged`,
            // `split`) may still hold the winning selector for this
            // path and direction. Falling through to the identity default
            // would misrepresent it as an untouched exact match instead of
            // correctly declining to resolve it; every other stage is
            // skipped too, since the winning selector already settled which
            // stage governs this path (ADR 0004: exact, then subtree, then
            // glob, in that order, never depending on what a later stage
            // might have said).
            if matches!(rule.rel, Rel::Merged | Rel::Split) {
                return None;
            }

            let fidelity = match direction {
                Direction::Forward => rule.fidelity_forward,
                Direction::Reverse => rule.fidelity_reverse,
            };

            // A `retyped` match refuses unconditionally regardless of its
            // declared fidelity: the shape change itself — not data loss —
            // is what this project's single-buffer read/write pipeline
            // cannot serve (ADR 0006). Its own fidelity is retained
            // verbatim for the loss log; the refusal is a separate signal.
            if rule.rel == Rel::Retyped {
                return Some(Self::explicit_match(
                    rule,
                    found.stage,
                    fidelity,
                    Outcome::Refusal(RefusalReason::UnservableRetype),
                ));
            }

            // `left_only`/`right_only` never carry a selector on the other
            // side (`ConversionMap::load` only ever registers one), so a
            // match here can never render a resolved path: the quantity is
            // either legitimately absent on the other side (`NoSource`), or
            // the artifact declares that direction outright unmappable.
            if matches!(rule.rel, Rel::LeftOnly | Rel::RightOnly) {
                let outcome = if fidelity == Fidelity::Unmappable {
                    Outcome::Refusal(RefusalReason::Unmappable)
                } else {
                    Outcome::NoSource
                };
                return Some(Self::explicit_match(rule, found.stage, fidelity, outcome));
            }

            // `Rel::Renamed` and `Rel::Moved` both carry one path on each
            // side, so they resolve through the same rendered-target path.
            if fidelity == Fidelity::Unmappable {
                return Some(Self::explicit_match(
                    rule,
                    found.stage,
                    fidelity,
                    Outcome::Refusal(RefusalReason::Unmappable),
                ));
            }

            let target = match direction {
                Direction::Forward => &rule.right,
                Direction::Reverse => &rule.left,
            };
            let target = target
                .as_ref()
                .expect("renamed or moved rule always carries both paths");
            let resolved_path = target.render(&found.suffix, &found.captures);
            let right_side_path = match direction {
                Direction::Forward => resolved_path.clone(),
                Direction::Reverse => path.to_string(),
            };

            if let Some(redefine_fidelity) = self.redefine_for(&right_side_path, direction) {
                return Some(Self::explicit_match(
                    rule,
                    found.stage,
                    redefine_fidelity,
                    Outcome::Refusal(RefusalReason::UnitRedefinition),
                ));
            }

            return Some(Self::explicit_match(
                rule,
                found.stage,
                fidelity,
                Outcome::Path {
                    value_transformation: self
                        .value_transformation_for(&right_side_path, direction),
                    resolved_path,
                },
            ));
        }

        if self.default_identical {
            // Identical mapping: the right-side spelling equals the path
            // itself regardless of which side supplied it.
            if let Some(redefine_fidelity) = self.redefine_for(path, direction) {
                return Some(Self::default_match(
                    redefine_fidelity,
                    Outcome::Refusal(RefusalReason::UnitRedefinition),
                ));
            }

            return Some(Self::default_match(
                Fidelity::Exact,
                Outcome::Path {
                    value_transformation: self.value_transformation_for(path, direction),
                    resolved_path: path.to_string(),
                },
            ));
        }

        None
    }

    /// Builds the [`RuleExplanation`] for an explicit rule match, sharing the
    /// match-identifying fields every outcome of a matched rule carries.
    fn explicit_match(
        rule: &Rule,
        stage: SelectorStage,
        fidelity: Fidelity,
        outcome: Outcome,
    ) -> RuleExplanation {
        RuleExplanation {
            rule_id: Some(rule.id.clone()),
            match_kind: MatchKind::Explicit,
            selector_stage: Some(stage),
            precedence: None,
            fidelity,
            outcome,
        }
    }

    /// Builds the [`RuleExplanation`] for a document-level identity default
    /// match; see [`Self::explicit_match`].
    fn default_match(fidelity: Fidelity, outcome: Outcome) -> RuleExplanation {
        RuleExplanation {
            rule_id: None,
            match_kind: MatchKind::Default,
            selector_stage: None,
            precedence: None,
            fidelity,
            outcome,
        }
    }

    /// The declared fidelity for `direction` if `right_side_path` falls
    /// under a `<redefine>` unit change, or `None` if no redefine claims it.
    /// Consulted after a right-side path is already known, the same way
    /// `value_transformation_for` is — a redefine is keyed on the right path
    /// and never competes with a structural rule for ownership of a path.
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
                        .map(|m| (m, entry.rule_index))
                })
                .max_by_key(|(m, _)| m.specificity);
            if let Some((m, rule_index)) = winner {
                return Some(BestMatch {
                    stage,
                    suffix: m.suffix,
                    captures: m.captures,
                    rule_index,
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
            Some((from_cocos, to_cocos)) => match direction {
                Direction::Forward => ValueTransformation::SignFlip {
                    from_cocos: from_cocos.clone(),
                    to_cocos: to_cocos.clone(),
                },
                Direction::Reverse => ValueTransformation::SignFlip {
                    from_cocos: to_cocos.clone(),
                    to_cocos: from_cocos.clone(),
                },
            },
            None => ValueTransformation::None,
        }
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
mod tests {
    use super::*;

    const APPROVED_ARTIFACT: &str = include_str!("../docs/3.39.0--4.1.1.xml");

    /// The resolved path of an explanation expected to be a translated path,
    /// panicking with the actual outcome otherwise — keeps assertions that
    /// only care about the path terse.
    fn resolved_path(explanation: &RuleExplanation) -> &str {
        match &explanation.outcome {
            Outcome::Path { resolved_path, .. } => resolved_path,
            other => panic!("expected a translated path, got {other:?}"),
        }
    }

    /// The value transformation of an explanation expected to be a
    /// translated path; see [`resolved_path`].
    fn value_transformation(explanation: &RuleExplanation) -> &ValueTransformation {
        match &explanation.outcome {
            Outcome::Path {
                value_transformation,
                ..
            } => value_transformation,
            other => panic!("expected a translated path, got {other:?}"),
        }
    }

    #[test]
    fn rejects_malformed_xml() {
        let err = ConversionMap::load("<not-xml").unwrap_err();
        assert!(matches!(err, LoadError::Xml(_)));
    }

    #[test]
    fn rejects_rule_missing_id() {
        let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule rel="renamed" left="a" right="b">
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        let err = ConversionMap::load(xml).unwrap_err();
        assert_eq!(
            err,
            LoadError::MissingAttribute {
                element: "rule".to_string(),
                attribute: "id".to_string(),
            }
        );
    }

    #[test]
    fn rejects_unknown_rel_value() {
        let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="bogus" rel="teleported" left="a" right="b">
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        let err = ConversionMap::load(xml).unwrap_err();
        assert_eq!(
            err,
            LoadError::UnknownValue {
                element: "rule".to_string(),
                attribute: "rel".to_string(),
                value: "teleported".to_string(),
            }
        );
    }

    #[test]
    fn rejects_duplicate_rule_id() {
        let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="dup" rel="renamed" left="a" right="b">
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
                <rule id="dup" rel="renamed" left="c" right="d">
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        let err = ConversionMap::load(xml).unwrap_err();
        assert_eq!(err, LoadError::DuplicateRuleId("dup".to_string()));
    }

    #[test]
    fn rejects_renamed_rules_with_the_same_source_path() {
        let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="first" rel="renamed" left="a" right="b">
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
                <rule id="second" rel="renamed" left="a" right="c">
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        let err = ConversionMap::load(xml).unwrap_err();
        assert_eq!(
            err,
            LoadError::DuplicateSourceSelector {
                role: "left",
                stage: SelectorStage::Exact,
                pattern: "a".to_string(),
            }
        );
    }

    #[test]
    fn rejects_renamed_rules_with_the_same_reverse_source_path() {
        let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="first" rel="renamed" left="a" right="b">
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
                <rule id="second" rel="renamed" left="c" right="b">
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        let err = ConversionMap::load(xml).unwrap_err();
        assert_eq!(
            err,
            LoadError::DuplicateSourceSelector {
                role: "right",
                stage: SelectorStage::Exact,
                pattern: "b".to_string(),
            }
        );
    }

    #[test]
    fn rejects_invalid_cocos_convention() {
        let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="not-a-convention"/>
              <side id="right" dd="4.1.1" cocos="17"/>
            </ids-map>
        "#;
        let err = ConversionMap::load(xml).unwrap_err();
        assert_eq!(
            err,
            LoadError::InvalidCocosConvention("not-a-convention".to_string())
        );
    }

    #[test]
    fn rejects_invalid_artifact_dd_version() {
        let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
            </ids-map>
        "#;
        let err = ConversionMap::load(xml).unwrap_err();
        assert_eq!(err, LoadError::InvalidArtifactDdVersion("3.39".to_string()));
    }

    #[test]
    fn rejects_rule_missing_fidelity() {
        let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="no-fidelity" rel="renamed" left="a" right="b"/>
              </rules>
            </ids-map>
        "#;
        let err = ConversionMap::load(xml).unwrap_err();
        assert_eq!(
            err,
            LoadError::InvalidRuleShape {
                rule_id: "no-fidelity".to_string(),
                reason: "missing required <fidelity> child".to_string(),
            }
        );
    }

    #[test]
    fn rejects_fidelity_with_invalid_value() {
        let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="bad-fidelity" rel="renamed" left="a" right="b">
                  <fidelity forward="perfect" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        let err = ConversionMap::load(xml).unwrap_err();
        assert_eq!(
            err,
            LoadError::UnknownValue {
                element: "fidelity".to_string(),
                attribute: "forward".to_string(),
                value: "perfect".to_string(),
            }
        );
    }

    #[test]
    fn rejects_renamed_rule_missing_right() {
        let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="half-rename" rel="renamed" left="a">
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        let err = ConversionMap::load(xml).unwrap_err();
        assert_eq!(
            err,
            LoadError::InvalidRuleShape {
                rule_id: "half-rename".to_string(),
                reason: "requires both `left` and `right`".to_string(),
            }
        );
    }

    #[test]
    fn rejects_merged_rule_with_duplicate_precedence() {
        let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="fold" rel="merged" right="b">
                  <from left="a1" precedence="1"/>
                  <from left="a2" precedence="1"/>
                  <fidelity forward="lossy" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        let err = ConversionMap::load(xml).unwrap_err();
        assert_eq!(
            err,
            LoadError::DuplicatePrecedence {
                rule_id: "fold".to_string(),
                precedence: 1,
            }
        );
    }

    #[test]
    fn rejects_merged_rule_without_from_entries() {
        let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="fold" rel="merged" right="b">
                  <fidelity forward="lossy" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        let err = ConversionMap::load(xml).unwrap_err();
        assert_eq!(
            err,
            LoadError::InvalidRuleShape {
                rule_id: "fold".to_string(),
                reason: "requires at least one <from left=\"...\"/>".to_string(),
            }
        );
    }

    #[test]
    fn rejects_duplicate_flip_path() {
        let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <transforms>
                <cocos from="11" to="17">
                  <flip path="p"/>
                </cocos>
                <cocos from="11" to="17">
                  <flip path="p"/>
                </cocos>
              </transforms>
            </ids-map>
        "#;
        let err = ConversionMap::load(xml).unwrap_err();
        assert_eq!(err, LoadError::DuplicateFlipPath("p".to_string()));
    }

    #[test]
    fn rejects_missing_side() {
        let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
            </ids-map>
        "#;
        let err = ConversionMap::load(xml).unwrap_err();
        assert_eq!(err, LoadError::MissingSide("right"));
    }

    #[test]
    fn loads_the_approved_equilibrium_artifact_as_one_complete_version_pair() {
        let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");
        assert_eq!(map.ids, "equilibrium");
        assert_eq!(map.left.dd, ArtifactDdVersion("3.39.0".to_string()));
        assert_eq!(map.left.cocos, CocosConvention("11".to_string()));
        assert_eq!(map.right.dd, ArtifactDdVersion("4.1.1".to_string()));
        assert_eq!(map.right.cocos, CocosConvention("17".to_string()));
        assert!(map.default_identical);
        // Sanity: both a merged rule and the lone renamed rule loaded.
        assert!(map.rules.iter().any(|r| r.id == "rename-beta-normal"));
        assert!(map.rules.iter().any(|r| r.id == "fold-constraints-j"));
    }

    #[test]
    fn identity_default_resolves_to_the_same_path_and_is_not_confused_with_unmatched() {
        let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");

        // Not claimed by any explicit <rule>: falls through to the document
        // default, which is a real match (Some), not a coincidental no-op.
        let explanation = map
            .resolve("vacuum_toroidal_field/b0", Direction::Forward)
            .expect("default-identical path must resolve");
        assert_eq!(explanation.match_kind, MatchKind::Default);
        assert_eq!(explanation.rule_id, None);
        assert_eq!(explanation.fidelity, Fidelity::Exact);
        assert_eq!(resolved_path(&explanation), "vacuum_toroidal_field/b0");
        assert_eq!(
            *value_transformation(&explanation),
            ValueTransformation::None
        );

        // A map with no document-level default must report a genuine miss as
        // `None`, distinct from the `Some(Default match)` case above.
        let no_default_xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
            </ids-map>
        "#;
        let no_default_map = ConversionMap::load(no_default_xml).unwrap();
        assert_eq!(
            no_default_map.resolve("vacuum_toroidal_field/b0", Direction::Forward),
            None
        );
    }

    #[test]
    fn renamed_rule_resolves_in_both_directions() {
        let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");

        let forward = map
            .resolve(
                "time_slice/global_quantities/beta_normal",
                Direction::Forward,
            )
            .expect("known DD3 rename must resolve forward");
        assert_eq!(forward.match_kind, MatchKind::Explicit);
        assert_eq!(forward.rule_id.as_deref(), Some("rename-beta-normal"));
        assert_eq!(forward.precedence, None);
        assert_eq!(forward.fidelity, Fidelity::Exact);
        assert_eq!(
            resolved_path(&forward),
            "time_slice/global_quantities/beta_tor_norm"
        );
        assert_eq!(*value_transformation(&forward), ValueTransformation::None);

        let reverse = map
            .resolve(
                "time_slice/global_quantities/beta_tor_norm",
                Direction::Reverse,
            )
            .expect("known DD4 name must resolve back to its DD3 spelling");
        assert_eq!(reverse.match_kind, MatchKind::Explicit);
        assert_eq!(reverse.rule_id.as_deref(), Some("rename-beta-normal"));
        assert_eq!(reverse.fidelity, Fidelity::Exact);
        assert_eq!(
            resolved_path(&reverse),
            "time_slice/global_quantities/beta_normal"
        );
        assert_eq!(*value_transformation(&reverse), ValueTransformation::None);
    }

    #[test]
    fn paths_claimed_by_a_not_yet_resolved_rule_kind_do_not_default_to_identity() {
        let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");

        // merged: this DD3 alias feeds a merged destination, not an identical one.
        assert_eq!(
            map.resolve("time_slice/constraints/j_tor", Direction::Forward),
            None
        );

        // split: this DD3 path feeds two DD4 destinations, not one identical one.
        assert_eq!(
            map.resolve("time_slice/global_quantities/psi_axis", Direction::Forward),
            None
        );
    }

    #[test]
    fn moved_rule_resolves_in_both_directions_with_its_declared_fidelity() {
        let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");

        let forward = map
            .resolve(
                "time_slice/boundary_separatrix/closest_wall_point/r",
                Direction::Forward,
            )
            .expect("the moved rule claims the left-side descendant");
        assert_eq!(forward.rule_id.as_deref(), Some("move-closest-wall-point"));
        assert_eq!(forward.fidelity, Fidelity::Exact);
        assert_eq!(
            resolved_path(&forward),
            "time_slice/boundary/closest_wall_point/r"
        );

        let reverse = map
            .resolve(
                "time_slice/boundary/closest_wall_point/r",
                Direction::Reverse,
            )
            .expect("the moved rule claims the right-side descendant");
        assert_eq!(reverse.rule_id.as_deref(), Some("move-closest-wall-point"));
        assert_eq!(reverse.fidelity, Fidelity::Exact);
        assert_eq!(
            resolved_path(&reverse),
            "time_slice/boundary_separatrix/closest_wall_point/r"
        );
    }

    #[test]
    fn left_only_relation_resolves_to_no_source_in_its_declared_lossy_direction() {
        let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");

        // drop-lcfs: this DD3 path was dropped, not carried through unchanged
        // — and not silently unmatched either, since a rule does claim it.
        let explanation = map
            .resolve("time_slice/boundary/lcfs", Direction::Forward)
            .expect("a left_only rule claims this path");
        assert_eq!(explanation.match_kind, MatchKind::Explicit);
        assert_eq!(explanation.rule_id.as_deref(), Some("drop-lcfs"));
        assert_eq!(explanation.fidelity, Fidelity::Lossy);
        assert_eq!(explanation.outcome, Outcome::NoSource);
    }

    #[test]
    fn right_only_relation_resolves_to_no_source_in_its_declared_lossy_direction() {
        let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");

        // new-contour-tree: this DD4-only path has no DD3 spelling to fall
        // back to — and not silently unmatched either.
        let explanation = map
            .resolve("time_slice/contour_tree", Direction::Reverse)
            .expect("a right_only rule claims this path");
        assert_eq!(explanation.match_kind, MatchKind::Explicit);
        assert_eq!(explanation.rule_id.as_deref(), Some("new-contour-tree"));
        assert_eq!(explanation.fidelity, Fidelity::Lossy);
        assert_eq!(explanation.outcome, Outcome::NoSource);
    }

    #[test]
    fn default_identity_match_surfaces_its_cocos_sign_flip() {
        let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");

        // Not claimed by any explicit <rule>, so it resolves via the
        // document default — but it does carry a value transformation.
        let forward = map
            .resolve("time_slice/boundary/psi", Direction::Forward)
            .expect("identical path must resolve");
        assert_eq!(forward.match_kind, MatchKind::Default);
        assert_eq!(
            *value_transformation(&forward),
            ValueTransformation::SignFlip {
                from_cocos: CocosConvention("11".to_string()),
                to_cocos: CocosConvention("17".to_string()),
            }
        );

        let reverse = map
            .resolve("time_slice/boundary/psi", Direction::Reverse)
            .expect("identical path must resolve in reverse too");
        assert_eq!(reverse.match_kind, MatchKind::Default);
        assert_eq!(
            *value_transformation(&reverse),
            ValueTransformation::SignFlip {
                from_cocos: CocosConvention("17".to_string()),
                to_cocos: CocosConvention("11".to_string()),
            }
        );
    }

    #[test]
    fn exact_selector_overrides_an_applicable_subtree_selector() {
        // Two independent renamed rules: one is an exact match for the full
        // requested path, the other is a subtree selector that also covers
        // it (its anchor is a strict prefix). The exact selector must win
        // regardless of which rule appears first in the document.
        let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="subtree-rule" rel="renamed"
                      left="a/b" right="x/y" subtree="yes">
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
                <rule id="exact-rule" rel="renamed" left="a/b/c" right="z">
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        let map = ConversionMap::load(xml).expect("both rules are structurally valid");

        let explanation = map
            .resolve("a/b/c", Direction::Forward)
            .expect("the exact rule must claim this path");
        assert_eq!(explanation.rule_id.as_deref(), Some("exact-rule"));
        assert_eq!(explanation.selector_stage, Some(SelectorStage::Exact));
        assert_eq!(resolved_path(&explanation), "z");
    }

    #[test]
    fn coverage_records_never_influence_resolution() {
        // The approved artifact's <coverage scope="time_slice/boundary" .../>
        // states forward="unmappable" — the opposite of what an identity
        // default would say. If resolution consulted coverage, this would
        // return Unmappable or None instead of the identity default's Exact.
        let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");
        let explanation = map
            .resolve("time_slice/boundary/type", Direction::Forward)
            .expect("path under a differently-verdicted coverage scope must still resolve");
        assert_eq!(explanation.match_kind, MatchKind::Default);
        assert_eq!(explanation.fidelity, Fidelity::Exact);
    }

    #[test]
    fn subtree_selector_resolves_the_anchor_itself_and_preserves_a_nested_suffix() {
        let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="move-group" rel="renamed"
                      left="a/old" right="a/new" subtree="yes">
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        let map = ConversionMap::load(xml).expect("subtree renamed rule is structurally valid");

        // The anchor itself, with no suffix to preserve.
        let anchor = map
            .resolve("a/old", Direction::Forward)
            .expect("the anchor path is itself covered by its own subtree rule");
        assert_eq!(anchor.selector_stage, Some(SelectorStage::Subtree));
        assert_eq!(resolved_path(&anchor), "a/new");

        // A nested descendant: the unmatched suffix must be preserved.
        let nested = map
            .resolve("a/old/leaf", Direction::Forward)
            .expect("a path nested under the subtree anchor must also resolve");
        assert_eq!(nested.selector_stage, Some(SelectorStage::Subtree));
        assert_eq!(resolved_path(&nested), "a/new/leaf");

        // Reverse direction must also preserve the suffix.
        let reverse = map
            .resolve("a/new/leaf", Direction::Reverse)
            .expect("the subtree rule is direction-neutral");
        assert_eq!(resolved_path(&reverse), "a/old/leaf");

        // A sibling that merely shares the anchor as a string prefix (not a
        // path-segment boundary) must not match.
        assert_eq!(map.resolve("a/oldish", Direction::Forward), None);
    }

    #[test]
    fn the_most_specific_applicable_subtree_selector_wins() {
        let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="shallow" rel="renamed"
                      left="a/b" right="x" subtree="yes">
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
                <rule id="deep" rel="renamed"
                      left="a/b/c" right="y" subtree="yes">
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        let map = ConversionMap::load(xml).expect("nested subtree anchors are not ambiguous");

        // Deeper than both anchors: the longer (more specific) one wins.
        let explanation = map
            .resolve("a/b/c/d", Direction::Forward)
            .expect("covered by the deeper subtree anchor");
        assert_eq!(explanation.rule_id.as_deref(), Some("deep"));
        assert_eq!(resolved_path(&explanation), "y/d");

        // Only the shallow anchor covers this one.
        let explanation = map
            .resolve("a/b/other", Direction::Forward)
            .expect("covered only by the shallow subtree anchor");
        assert_eq!(explanation.rule_id.as_deref(), Some("shallow"));
        assert_eq!(resolved_path(&explanation), "x/other");
    }

    #[test]
    fn duplicate_subtree_anchors_on_the_same_source_role_invalidate_the_map() {
        let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="first" rel="renamed" left="a" right="b" subtree="yes">
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
                <rule id="second" rel="left_only" left="a" subtree="yes">
                  <fidelity forward="lossy" reverse="unmappable"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        let err = ConversionMap::load(xml).unwrap_err();
        assert_eq!(
            err,
            LoadError::DuplicateSourceSelector {
                role: "left",
                stage: SelectorStage::Subtree,
                pattern: "a".to_string(),
            }
        );
    }

    #[test]
    fn glob_selector_resolves_only_when_no_exact_or_subtree_selector_applies() {
        let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="glob-rule" rel="renamed"
                      left="constraints/*/measured" right="constraints/*/value" glob="yes">
                  <fidelity forward="lossy" reverse="lossy"/>
                </rule>
                <rule id="exact-rule" rel="renamed"
                      left="constraints/ip/measured" right="constraints/ip/exact-value">
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        let map = ConversionMap::load(xml).expect("glob and exact rules here do not conflict");

        // No exact or subtree selector claims this one: the glob fallback applies.
        let glob_match = map
            .resolve("constraints/flux_loop/measured", Direction::Forward)
            .expect("the glob rule must claim this path");
        assert_eq!(glob_match.rule_id.as_deref(), Some("glob-rule"));
        assert_eq!(glob_match.selector_stage, Some(SelectorStage::Glob));
        assert_eq!(resolved_path(&glob_match), "constraints/flux_loop/value");

        // An exact selector for the very same glob-matchable path must win instead.
        let exact_match = map
            .resolve("constraints/ip/measured", Direction::Forward)
            .expect("the exact rule must claim this path over the glob fallback");
        assert_eq!(exact_match.rule_id.as_deref(), Some("exact-rule"));
        assert_eq!(exact_match.selector_stage, Some(SelectorStage::Exact));
        assert_eq!(resolved_path(&exact_match), "constraints/ip/exact-value");

        // A path with a different segment count than the glob pattern is not covered.
        assert_eq!(
            map.resolve("constraints/ip/position/measured", Direction::Forward),
            None
        );
    }

    #[test]
    fn overlapping_glob_selectors_on_the_same_source_role_invalidate_the_map() {
        let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="first" rel="left_only" left="a/*/c" glob="yes">
                  <fidelity forward="lossy" reverse="unmappable"/>
                </rule>
                <rule id="second" rel="left_only" left="a/b/*" glob="yes">
                  <fidelity forward="lossy" reverse="unmappable"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        let err = ConversionMap::load(xml).unwrap_err();
        assert_eq!(
            err,
            LoadError::OverlappingSourceSelectors {
                role: "left",
                first: "a/*/c".to_string(),
                second: "a/b/*".to_string(),
            }
        );
    }

    #[test]
    fn rejects_a_rule_that_sets_both_subtree_and_glob() {
        let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="bogus" rel="renamed" left="a" right="b" subtree="yes" glob="yes">
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        let err = ConversionMap::load(xml).unwrap_err();
        assert_eq!(
            err,
            LoadError::InvalidRuleShape {
                rule_id: "bogus".to_string(),
                reason: "must not set both `subtree` and `glob`".to_string(),
            }
        );
    }

    #[test]
    fn rejects_a_glob_renamed_rule_whose_sides_carry_different_wildcard_counts() {
        let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="lopsided" rel="renamed"
                      left="constraints/*/measured" right="constraints/value" glob="yes">
                  <fidelity forward="lossy" reverse="lossy"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        let err = ConversionMap::load(xml).unwrap_err();
        assert_eq!(
            err,
            LoadError::InvalidRuleShape {
                rule_id: "lopsided".to_string(),
                reason: "glob `left` and `right` must carry the same number of `*` wildcards"
                    .to_string(),
            }
        );
    }

    #[test]
    fn approved_artifact_subtree_rules_claim_their_whole_descendant_paths() {
        // `drop-lcfs` is `left_only`, `subtree="yes"` on
        // `time_slice/boundary/lcfs`. It must be recognised as claiming
        // every path nested under its anchor, or such a path would
        // incorrectly fall through to the document-level identity default
        // instead of correctly resolving to no source.
        let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");
        let explanation = map
            .resolve("time_slice/boundary/lcfs/r", Direction::Forward)
            .expect("the drop-lcfs subtree rule claims this nested path too");
        assert_eq!(explanation.rule_id.as_deref(), Some("drop-lcfs"));
        assert_eq!(explanation.outcome, Outcome::NoSource);
    }

    #[test]
    fn rank_changing_retype_resolves_to_a_refusal_plan() {
        let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");

        // grids_ggd/grid/space/coordinates_type: DD3's INT_1D becomes DD4's
        // STRUCT_ARRAY (ADR 0006). The shim cannot serve this without
        // fabricating an arraystruct context, so it must refuse before any
        // IMAS-Core call is possible — regardless of the rule's own exact
        // fidelity, which describes the data, not the seam limitation.
        let forward = map
            .resolve("grids_ggd/grid/space/coordinates_type", Direction::Forward)
            .expect("the retype rule claims this path");
        assert_eq!(forward.match_kind, MatchKind::Explicit);
        assert_eq!(forward.rule_id.as_deref(), Some("retype-coordinates-type"));
        assert_eq!(forward.fidelity, Fidelity::Exact);
        assert_eq!(
            forward.outcome,
            Outcome::Refusal(RefusalReason::UnservableRetype)
        );

        // The subtree anchor covers descendants of the retyped container too.
        let nested = map
            .resolve(
                "grids_ggd/grid/space/coordinates_type/index",
                Direction::Reverse,
            )
            .expect("the retype rule's subtree anchor claims this nested path");
        assert_eq!(nested.rule_id.as_deref(), Some("retype-coordinates-type"));
        assert_eq!(
            nested.outcome,
            Outcome::Refusal(RefusalReason::UnservableRetype)
        );
    }

    #[test]
    fn unit_redefinition_resolves_to_a_refusal_plan() {
        let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");

        // Not claimed by any structural <rule>, so it would otherwise
        // resolve via the document-level identity default — but the m ->
        // m^-2 redefinition means the shim cannot recover the variance
        // needed to invert it, so it must refuse instead.
        let explanation = map
            .resolve(
                "time_slice/constraints/strike_point/chi_squared_r",
                Direction::Forward,
            )
            .expect("the path resolves, to a refusal rather than a translation");
        assert_eq!(explanation.match_kind, MatchKind::Default);
        assert_eq!(explanation.fidelity, Fidelity::Unmappable);
        assert_eq!(
            explanation.outcome,
            Outcome::Refusal(RefusalReason::UnitRedefinition)
        );

        let reverse = map
            .resolve(
                "time_slice/constraints/strike_point/chi_squared_r",
                Direction::Reverse,
            )
            .expect("the redefinition refuses in both directions");
        assert_eq!(
            reverse.outcome,
            Outcome::Refusal(RefusalReason::UnitRedefinition)
        );
    }

    #[test]
    fn left_only_relation_resolves_to_refusal_when_declared_unmappable() {
        // Same rel kind and matching direction as the approved artifact's
        // `drop-lcfs`, but this artifact declares the matching (forward)
        // direction unmappable rather than lossy: the outcome must follow
        // what the artifact declares, not the rel kind alone.
        let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="drop-unrecoverable" rel="left_only" left="a/b">
                  <fidelity forward="unmappable" reverse="unmappable"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        let map = ConversionMap::load(xml).expect("left_only rule is structurally valid");

        let explanation = map
            .resolve("a/b", Direction::Forward)
            .expect("the left_only rule claims this path");
        assert_eq!(explanation.fidelity, Fidelity::Unmappable);
        assert_eq!(
            explanation.outcome,
            Outcome::Refusal(RefusalReason::Unmappable)
        );
    }

    #[test]
    fn explicit_unmappable_relation_is_distinct_from_an_unmatched_path() {
        // A path an explicit rule claims as unmappable must still resolve
        // (Some), never coincide with the None a genuinely unclaimed path
        // gets when the artifact declares no identity default.
        let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="unrecoverable-rename" rel="renamed" left="a" right="b">
                  <fidelity forward="unmappable" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        let map = ConversionMap::load(xml).expect("renamed rule is structurally valid");

        let claimed = map
            .resolve("a", Direction::Forward)
            .expect("an explicit rule claims this path, even though it is unmappable");
        assert_eq!(claimed.outcome, Outcome::Refusal(RefusalReason::Unmappable));

        assert_eq!(map.resolve("unclaimed/path", Direction::Forward), None);
    }
}
