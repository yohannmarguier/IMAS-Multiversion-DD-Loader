//! Conversion-map artifact loading and direction-neutral path resolution.
//!
//! See `docs/adr/0004-xml-conversion-map-artifact.md` and CONTEXT.md's
//! "conversion-map artifact", "rule explanation", "path-level rule" and
//! "glob" entries. This module parses the hand-authored equilibrium 3.39.0
//! ⇄ 4.1.1 artifact when supplied by its caller, and resolves the
//! document-level identity default, `renamed` path-level rules, and
//! `merged`/`split` rules — matched through any of the three selector stages
//! ADR 0004 defines (`Exact`, `Subtree`, `Glob`, tried in that order; see
//! [`ConversionMap::best_match`] and `Selector::try_match`). A `merged` or
//! `split` rule's ambiguous direction (the side with more than one path)
//! resolves to an ordered [`CandidatePath`] read plan rather than a single
//! path, per ADR 0006: every declared source, in precedence order, for later
//! read execution to try in turn without reading data or re-deriving that
//! order itself (#48). The remaining `rel` kinds (`moved`, `retyped`,
//! `left_only`, `right_only`) parse structurally and participate in selector
//! matching — so a path any of them claims is never misreported as an
//! unmatched, defaulted-to-identity path — but `resolve` does not yet turn a
//! match on one of them into a translated path or an explicit refusal; a
//! later issue extends those resolution outcomes (#49).
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

/// Test information identifying the rule selected for a requested DD path,
/// its match kind, precedence, path result, fidelity and value
/// transformation (CONTEXT.md's "rule explanation").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleExplanation {
    /// The selected rule's id, or `None` for a `Default` match.
    pub rule_id: Option<String>,
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
    /// The primary resolved path: the only path for an unambiguous
    /// resolution, or the precedence-first entry of `candidates` when this
    /// resolution is a read plan.
    pub resolved_path: String,
    pub fidelity: Fidelity,
    /// The value transformation for `resolved_path` specifically — equal to
    /// `candidates[0].value_transformation` when `candidates` is populated.
    pub value_transformation: ValueTransformation,
    /// The full ordered read plan for a `merged` rule resolved in reverse or
    /// a `split` rule resolved forward — the side with more than one
    /// possible source, where the shim cannot pick a single winner without
    /// reading data (ADR 0006). Empty for every other resolution, including
    /// a `merged` rule resolved forward and a `split` rule resolved in
    /// reverse, which each have exactly one destination and so need no plan.
    pub candidates: Vec<CandidatePath>,
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
                    parse_transforms(&child, &mut sign_flips)?;
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

        Ok(ConversionMap {
            ids,
            left,
            right,
            default_identical,
            rules,
            sign_flips,
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

            return match rule.rel {
                Rel::Renamed => Some(self.resolve_renamed(rule, path, direction, &found)),
                Rel::Merged => Some(self.resolve_merged(rule, path, direction, &found)),
                Rel::Split => Some(self.resolve_split(rule, path, direction, &found)),
                // A rule of a kind not yet resolved (`moved`, `retyped`,
                // `left_only`, `right_only`) may still hold the winning
                // selector for this path and direction. Falling through to
                // the identity default would misrepresent it as an
                // untouched exact match instead of correctly declining to
                // resolve it; every other stage is skipped too, since the
                // winning selector already settled which stage governs this
                // path (ADR 0004: exact, then subtree, then glob, in that
                // order, never depending on what a later stage might have
                // said). A later issue extends these outcomes (#49).
                Rel::Moved | Rel::Retyped | Rel::LeftOnly | Rel::RightOnly => None,
            };
        }

        if self.default_identical {
            let resolved_path = path.to_string();
            // Identical mapping: the right-side spelling equals the path
            // itself regardless of which side supplied it.
            return Some(RuleExplanation {
                rule_id: None,
                match_kind: MatchKind::Default,
                selector_stage: None,
                precedence: None,
                value_transformation: self.value_transformation_for(path, direction),
                resolved_path,
                fidelity: Fidelity::Exact,
                candidates: Vec::new(),
            });
        }

        None
    }

    /// Resolves a `renamed` rule's single, unambiguous path on the other side.
    fn resolve_renamed(
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
            .expect("renamed rule always carries both paths");
        let resolved_path = target.render(&found.suffix, &found.captures);
        let right_side_path = match direction {
            Direction::Forward => resolved_path.clone(),
            Direction::Reverse => path.to_string(),
        };
        RuleExplanation {
            rule_id: Some(rule.id.clone()),
            match_kind: MatchKind::Explicit,
            selector_stage: Some(found.stage),
            precedence: None,
            value_transformation: self.value_transformation_for(&right_side_path, direction),
            resolved_path,
            fidelity,
            candidates: Vec::new(),
        }
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
                let value_transformation = self.value_transformation_for(&resolved_path, direction);
                RuleExplanation {
                    rule_id: Some(rule.id.clone()),
                    match_kind: MatchKind::Explicit,
                    selector_stage: Some(found.stage),
                    precedence: found.precedence,
                    value_transformation,
                    resolved_path,
                    fidelity: rule.fidelity_forward,
                    candidates: Vec::new(),
                }
            }
            Direction::Reverse => {
                // The canonical path was already supplied, so it is the
                // right-side path for every candidate alike.
                let candidates =
                    self.candidate_paths(&rule.froms, found, direction, |_candidate| {
                        path.to_string()
                    });
                let resolved_path = candidates[0].path.clone();
                let value_transformation = candidates[0].value_transformation.clone();
                RuleExplanation {
                    rule_id: Some(rule.id.clone()),
                    match_kind: MatchKind::Explicit,
                    selector_stage: Some(found.stage),
                    precedence: None,
                    resolved_path,
                    fidelity: rule.fidelity_reverse,
                    value_transformation,
                    candidates,
                }
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
                let value_transformation = candidates[0].value_transformation.clone();
                RuleExplanation {
                    rule_id: Some(rule.id.clone()),
                    match_kind: MatchKind::Explicit,
                    selector_stage: Some(found.stage),
                    precedence: None,
                    resolved_path,
                    fidelity: rule.fidelity_forward,
                    value_transformation,
                    candidates,
                }
            }
            Direction::Reverse => {
                let target = rule.left.as_ref().expect("split rule has a left path");
                let resolved_path = target.render(&found.suffix, &found.captures);
                let value_transformation = self.value_transformation_for(path, direction);
                RuleExplanation {
                    rule_id: Some(rule.id.clone()),
                    match_kind: MatchKind::Explicit,
                    selector_stage: Some(found.stage),
                    precedence: found.precedence,
                    value_transformation,
                    resolved_path,
                    fidelity: rule.fidelity_reverse,
                    candidates: Vec::new(),
                }
            }
        }
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
                required_attr(&child, "redefine", "glob")?;
                required_attr(&child, "redefine", "left-units")?;
                required_attr(&child, "redefine", "right-units")?;
                let fidelity_node = child
                    .children()
                    .find(|n| n.is_element() && n.tag_name().name() == "fidelity")
                    .ok_or_else(|| LoadError::MissingAttribute {
                        element: "redefine".to_string(),
                        attribute: "fidelity".to_string(),
                    })?;
                parse_fidelity_value(&fidelity_node, "forward")?;
                parse_fidelity_value(&fidelity_node, "reverse")?;
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
        assert_eq!(explanation.resolved_path, "vacuum_toroidal_field/b0");
        assert_eq!(explanation.fidelity, Fidelity::Exact);
        assert_eq!(explanation.value_transformation, ValueTransformation::None);

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
        assert_eq!(
            forward.resolved_path,
            "time_slice/global_quantities/beta_tor_norm"
        );
        assert_eq!(forward.fidelity, Fidelity::Exact);
        assert_eq!(forward.value_transformation, ValueTransformation::None);

        let reverse = map
            .resolve(
                "time_slice/global_quantities/beta_tor_norm",
                Direction::Reverse,
            )
            .expect("known DD4 name must resolve back to its DD3 spelling");
        assert_eq!(reverse.match_kind, MatchKind::Explicit);
        assert_eq!(reverse.rule_id.as_deref(), Some("rename-beta-normal"));
        assert_eq!(
            reverse.resolved_path,
            "time_slice/global_quantities/beta_normal"
        );
        assert_eq!(reverse.fidelity, Fidelity::Exact);
        assert_eq!(reverse.value_transformation, ValueTransformation::None);
    }

    #[test]
    fn paths_claimed_by_a_not_yet_resolved_rule_kind_do_not_default_to_identity() {
        let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");

        // left_only: this DD3 path was dropped, not carried through unchanged.
        assert_eq!(
            map.resolve("time_slice/boundary/lcfs", Direction::Forward),
            None
        );

        // moved: this DD3 path relocated under `boundary`, not kept in place.
        assert_eq!(
            map.resolve("time_slice/boundary_separatrix/gap", Direction::Forward),
            None
        );

        // right_only: this DD4-only path has no DD3 spelling to fall back to.
        assert_eq!(
            map.resolve("time_slice/contour_tree", Direction::Reverse),
            None
        );
    }

    #[test]
    fn merged_rule_returns_all_candidates_in_declared_precedence_order() {
        let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");

        // fold-p2d-bphi is a three-source merged rule: b_field_phi
        // (precedence 1), b_field_tor (precedence 2), and b_tor (precedence
        // 3) all fold into the one DD4 canonical path. Requesting the
        // canonical path in reverse must return every candidate, in
        // declared precedence order, for read execution to try in turn
        // (ADR 0006) — not just the first hit, since resolution never reads
        // data.
        let explanation = map
            .resolve("time_slice/profiles_2d/b_field_phi", Direction::Reverse)
            .expect("merged rule's canonical path must resolve in reverse");
        assert_eq!(explanation.match_kind, MatchKind::Explicit);
        assert_eq!(explanation.rule_id.as_deref(), Some("fold-p2d-bphi"));
        assert_eq!(explanation.fidelity, Fidelity::Exact);
        assert_eq!(explanation.precedence, None);

        let candidates: Vec<(&str, u32)> = explanation
            .candidates
            .iter()
            .map(|c| (c.path.as_str(), c.precedence))
            .collect();
        assert_eq!(
            candidates,
            vec![
                ("time_slice/profiles_2d/b_field_phi", 1),
                ("time_slice/profiles_2d/b_field_tor", 2),
                ("time_slice/profiles_2d/b_tor", 3),
            ]
        );
        // The precedence-first candidate is surfaced as the primary result.
        assert_eq!(
            explanation.resolved_path,
            "time_slice/profiles_2d/b_field_phi"
        );
    }

    #[test]
    fn merged_precedence_is_independent_of_xml_document_order() {
        let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="fold" rel="merged" right="right/canonical">
                  <from left="left/second" precedence="2"/>
                  <from left="left/first" precedence="1"/>
                  <fidelity forward="lossy" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        let map =
            ConversionMap::load(xml).expect("declared precedence need not follow document order");

        let explanation = map
            .resolve("right/canonical", Direction::Reverse)
            .expect("merged rule must resolve in reverse");
        let paths: Vec<&str> = explanation
            .candidates
            .iter()
            .map(|c| c.path.as_str())
            .collect();
        assert_eq!(paths, vec!["left/first", "left/second"]);
    }

    #[test]
    fn merged_rule_resolves_forward_to_its_single_canonical_target() {
        let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");

        // A deprecated DD3 alias feeding a merged rule resolves forward to
        // the one DD4 canonical path; there is no ambiguity to defer to
        // read time, so no candidate plan is needed.
        let explanation = map
            .resolve("time_slice/constraints/j_tor", Direction::Forward)
            .expect("a merged rule's DD3 alias must resolve forward");
        assert_eq!(explanation.match_kind, MatchKind::Explicit);
        assert_eq!(explanation.rule_id.as_deref(), Some("fold-constraints-j"));
        assert_eq!(explanation.resolved_path, "time_slice/constraints/j_phi");
        assert_eq!(explanation.precedence, Some(2));
        assert_eq!(explanation.fidelity, Fidelity::Lossy);
        assert!(explanation.candidates.is_empty());
    }

    #[test]
    fn split_rule_resolves_forward_to_all_candidate_destinations_in_precedence_order() {
        let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");

        let explanation = map
            .resolve("time_slice/global_quantities/psi_axis", Direction::Forward)
            .expect("split rule's DD3 source must resolve forward");
        assert_eq!(explanation.match_kind, MatchKind::Explicit);
        assert_eq!(explanation.rule_id.as_deref(), Some("split-psi-axis"));
        assert_eq!(explanation.fidelity, Fidelity::Exact);
        assert_eq!(explanation.precedence, None);
        assert_eq!(
            explanation.resolved_path,
            "time_slice/global_quantities/psi_axis"
        );

        let candidates: Vec<(&str, u32)> = explanation
            .candidates
            .iter()
            .map(|c| (c.path.as_str(), c.precedence))
            .collect();
        assert_eq!(
            candidates,
            vec![
                ("time_slice/global_quantities/psi_axis", 1),
                ("time_slice/global_quantities/psi_magnetic_axis", 2),
            ]
        );
        // Both destinations are separately declared `<flip>` targets; each
        // candidate must carry its own value transformation rather than one
        // shared for the whole rule.
        for candidate in &explanation.candidates {
            assert_eq!(
                candidate.value_transformation,
                ValueTransformation::SignFlip {
                    from_cocos: CocosConvention("11".to_string()),
                    to_cocos: CocosConvention("17".to_string()),
                }
            );
        }
    }

    #[test]
    fn split_rule_resolves_reverse_to_its_single_source_with_matched_precedence() {
        let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");

        let explanation = map
            .resolve(
                "time_slice/global_quantities/psi_magnetic_axis",
                Direction::Reverse,
            )
            .expect("split rule's DD4 destination must resolve back to the single DD3 source");
        assert_eq!(explanation.match_kind, MatchKind::Explicit);
        assert_eq!(explanation.rule_id.as_deref(), Some("split-psi-axis"));
        assert_eq!(
            explanation.resolved_path,
            "time_slice/global_quantities/psi_axis"
        );
        assert_eq!(explanation.precedence, Some(2));
        assert_eq!(explanation.fidelity, Fidelity::Exact);
        assert_eq!(
            explanation.value_transformation,
            ValueTransformation::SignFlip {
                from_cocos: CocosConvention("17".to_string()),
                to_cocos: CocosConvention("11".to_string()),
            }
        );
        assert!(explanation.candidates.is_empty());
    }

    #[test]
    fn merged_and_split_rules_retain_independently_declared_forward_and_reverse_fidelity() {
        let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");

        let merged_forward = map
            .resolve("time_slice/constraints/j_tor", Direction::Forward)
            .expect("merged rule must resolve forward");
        let merged_reverse = map
            .resolve("time_slice/constraints/j_phi", Direction::Reverse)
            .expect("merged rule must resolve reverse");
        assert_eq!(merged_forward.fidelity, Fidelity::Lossy);
        assert_eq!(merged_reverse.fidelity, Fidelity::Exact);

        let split_forward = map
            .resolve("time_slice/global_quantities/psi_axis", Direction::Forward)
            .expect("split rule must resolve forward");
        let split_reverse = map
            .resolve(
                "time_slice/global_quantities/psi_magnetic_axis",
                Direction::Reverse,
            )
            .expect("split rule must resolve reverse");
        assert_eq!(split_forward.fidelity, Fidelity::Exact);
        assert_eq!(split_reverse.fidelity, Fidelity::Exact);
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
            forward.value_transformation,
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
            reverse.value_transformation,
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
        assert_eq!(explanation.resolved_path, "z");
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
        assert_eq!(anchor.resolved_path, "a/new");

        // A nested descendant: the unmatched suffix must be preserved.
        let nested = map
            .resolve("a/old/leaf", Direction::Forward)
            .expect("a path nested under the subtree anchor must also resolve");
        assert_eq!(nested.selector_stage, Some(SelectorStage::Subtree));
        assert_eq!(nested.resolved_path, "a/new/leaf");

        // Reverse direction must also preserve the suffix.
        let reverse = map
            .resolve("a/new/leaf", Direction::Reverse)
            .expect("the subtree rule is direction-neutral");
        assert_eq!(reverse.resolved_path, "a/old/leaf");

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
        assert_eq!(explanation.resolved_path, "y/d");

        // Only the shallow anchor covers this one.
        let explanation = map
            .resolve("a/b/other", Direction::Forward)
            .expect("covered only by the shallow subtree anchor");
        assert_eq!(explanation.rule_id.as_deref(), Some("shallow"));
        assert_eq!(explanation.resolved_path, "x/other");
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
        assert_eq!(glob_match.resolved_path, "constraints/flux_loop/value");

        // An exact selector for the very same glob-matchable path must win instead.
        let exact_match = map
            .resolve("constraints/ip/measured", Direction::Forward)
            .expect("the exact rule must claim this path over the glob fallback");
        assert_eq!(exact_match.rule_id.as_deref(), Some("exact-rule"));
        assert_eq!(exact_match.selector_stage, Some(SelectorStage::Exact));
        assert_eq!(exact_match.resolved_path, "constraints/ip/exact-value");

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
        // `time_slice/boundary/lcfs`. A rule kind this issue does not resolve
        // still must be recognised as claiming every path nested under its
        // anchor, or such a path would incorrectly fall through to the
        // document-level identity default instead of correctly declining to
        // resolve.
        let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");
        assert_eq!(
            map.resolve("time_slice/boundary/lcfs/r", Direction::Forward),
            None
        );
    }
}
