//! Conversion-map artifact loading and direction-neutral path resolution.
//!
//! See `docs/adr/0004-xml-conversion-map-artifact.md` and CONTEXT.md's
//! "conversion-map artifact", "rule explanation" and "path-level rule"
//! entries. This module parses the hand-authored equilibrium 3.39.0 ⇄ 4.1.1
//! artifact when supplied by its caller, and resolves the document-level
//! identity default and `renamed` path-level rules. Other `rel` kinds
//! (`merged`, `moved`, `retyped`, `split`, `left_only`, `right_only`) parse
//! structurally, so the artifact loads as one complete unit, but `resolve`
//! does not yet match them — later issues extend match kinds (#47) and
//! resolution outcomes (#48, #49).
//!
//! `<include>` and `<coverage>` elements are recognised and skipped: the
//! included `../common/*.xml` and `../inventory/*.txt` files are a future
//! generator concern (ADR 0004), and coverage records are generated
//! documentation that must never influence resolution (CONTEXT.md's
//! "coverage record").

use std::collections::{HashMap, HashSet};
use std::fmt;

use roxmltree::Document;

/// One side of a conversion-map artifact: a DD version and its COCOS convention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Side {
    pub dd: String,
    pub cocos: String,
}

/// Which side of the map a resolution request travels from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Left DD path supplied, resolve to the right DD's spelling.
    Forward,
    /// Right DD path supplied, resolve to the left DD's spelling.
    Reverse,
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
        from_cocos: String,
        to_cocos: String,
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
    pub path: String,
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
    pub left: Option<String>,
    pub right: Option<String>,
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

/// Test information identifying the rule selected for a requested DD path,
/// its match kind, precedence, path result, fidelity and value
/// transformation (CONTEXT.md's "rule explanation").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleExplanation {
    /// The selected rule's id, or `None` for a `Default` match.
    pub rule_id: Option<String>,
    pub match_kind: MatchKind,
    /// The winning source's precedence within its rule, where applicable
    /// (a `merged`/`split` `<from>` entry). Always `None` for the match
    /// kinds this issue resolves.
    pub precedence: Option<u32>,
    pub resolved_path: String,
    pub fidelity: Fidelity,
    pub value_transformation: ValueTransformation,
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
    DuplicatePrecedence {
        rule_id: String,
        precedence: u32,
    },
    InvalidRuleShape {
        rule_id: String,
        reason: String,
    },
    DuplicateFlipPath(String),
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
            LoadError::MissingSide(id) => write!(f, "missing required <side id=\"{id}\"/>"),
        }
    }
}

impl std::error::Error for LoadError {}

/// A loaded conversion-map artifact for one adjacent DD-version step
/// (CONTEXT.md's "conversion-map artifact").
#[derive(Debug, Clone)]
pub struct ConversionMap {
    pub ids: String,
    pub left: Side,
    pub right: Side,
    pub default_identical: bool,
    rules: Vec<Rule>,
    sign_flips: HashMap<String, (String, String)>,
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
        let mut rules = Vec::new();
        let mut sign_flips: HashMap<String, (String, String)> = HashMap::new();
        let mut seen_rule_ids: HashSet<String> = HashSet::new();

        for child in root.children().filter(|n| n.is_element()) {
            match child.tag_name().name() {
                "side" => {
                    let id = required_attr(&child, "side", "id")?;
                    let dd = required_attr(&child, "side", "dd")?.to_string();
                    let cocos = required_attr(&child, "side", "cocos")?.to_string();
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

        Ok(ConversionMap {
            ids,
            left,
            right,
            default_identical,
            rules,
            sign_flips,
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
        for rule in &self.rules {
            if rule.rel != Rel::Renamed {
                continue;
            }
            let (from, to, fidelity) = match direction {
                Direction::Forward => (
                    rule.left.as_deref(),
                    rule.right.as_deref(),
                    rule.fidelity_forward,
                ),
                Direction::Reverse => (
                    rule.right.as_deref(),
                    rule.left.as_deref(),
                    rule.fidelity_reverse,
                ),
            };
            if from == Some(path) {
                let resolved_path = to
                    .expect("renamed rule always carries both paths")
                    .to_string();
                let right_side_path = match direction {
                    Direction::Forward => resolved_path.clone(),
                    Direction::Reverse => path.to_string(),
                };
                return Some(RuleExplanation {
                    rule_id: Some(rule.id.clone()),
                    match_kind: MatchKind::Explicit,
                    precedence: None,
                    value_transformation: self.value_transformation_for(&right_side_path),
                    resolved_path,
                    fidelity,
                });
            }
        }

        // A rule of a kind this issue does not yet resolve (`merged`, `moved`,
        // `retyped`, `split`, `left_only`, `right_only`) may still claim this
        // path on the requested direction's source side. Falling through to
        // the identity default would misrepresent it as an untouched exact
        // match instead of correctly declining to resolve it.
        if self.some_rule_claims(path, direction) {
            return None;
        }

        if self.default_identical {
            let resolved_path = path.to_string();
            // Identical mapping: the right-side spelling equals the path
            // itself regardless of which side supplied it.
            return Some(RuleExplanation {
                rule_id: None,
                match_kind: MatchKind::Default,
                precedence: None,
                value_transformation: self.value_transformation_for(path),
                resolved_path,
                fidelity: Fidelity::Exact,
            });
        }

        None
    }

    /// True when some rule states that `path` exists on `direction`'s source
    /// side, regardless of whether `resolve` knows how to interpret that
    /// rule's `rel` yet. `Renamed` is included for completeness even though
    /// `resolve` already matches and returns on it before this is reached.
    fn some_rule_claims(&self, path: &str, direction: Direction) -> bool {
        self.rules.iter().any(|rule| match (rule.rel, direction) {
            (Rel::Renamed | Rel::Moved | Rel::Retyped, Direction::Forward) => {
                rule.left.as_deref() == Some(path)
            }
            (Rel::Renamed | Rel::Moved | Rel::Retyped, Direction::Reverse) => {
                rule.right.as_deref() == Some(path)
            }
            (Rel::LeftOnly, Direction::Forward) => rule.left.as_deref() == Some(path),
            (Rel::LeftOnly, Direction::Reverse) => false,
            (Rel::RightOnly, Direction::Forward) => false,
            (Rel::RightOnly, Direction::Reverse) => rule.right.as_deref() == Some(path),
            (Rel::Merged, Direction::Forward) => rule.froms.iter().any(|f| f.path == path),
            (Rel::Merged, Direction::Reverse) => rule.right.as_deref() == Some(path),
            (Rel::Split, Direction::Forward) => rule.left.as_deref() == Some(path),
            (Rel::Split, Direction::Reverse) => rule.froms.iter().any(|f| f.path == path),
        })
    }

    fn value_transformation_for(&self, right_side_path: &str) -> ValueTransformation {
        match self.sign_flips.get(right_side_path) {
            Some((from_cocos, to_cocos)) => ValueTransformation::SignFlip {
                from_cocos: from_cocos.clone(),
                to_cocos: to_cocos.clone(),
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

fn parse_froms(
    rule_id: &str,
    rule_node: &roxmltree::Node,
    side_attr: &str,
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
        froms.push(FromEntry { path, precedence });
    }
    Ok(froms)
}

fn parse_rule(node: &roxmltree::Node) -> Result<Rule, LoadError> {
    let id = required_attr(node, "rule", "id")?.to_string();
    let rel = parse_rel(node)?;
    let left = node.attribute("left").map(str::to_string);
    let right = node.attribute("right").map(str::to_string);
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
            let froms = parse_froms(&id, node, "left")?;
            if !froms.is_empty() {
                return Err(shape_error("must not carry <from> children"));
            }
            froms
        }
        Rel::LeftOnly => {
            if left.is_none() || right.is_some() {
                return Err(shape_error("requires `left` only"));
            }
            let froms = parse_froms(&id, node, "left")?;
            if !froms.is_empty() {
                return Err(shape_error("must not carry <from> children"));
            }
            froms
        }
        Rel::RightOnly => {
            if right.is_none() || left.is_some() {
                return Err(shape_error("requires `right` only"));
            }
            let froms = parse_froms(&id, node, "right")?;
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
            let froms = parse_froms(&id, node, "left")?;
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
            let froms = parse_froms(&id, node, "right")?;
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
    sign_flips: &mut HashMap<String, (String, String)>,
) -> Result<(), LoadError> {
    for child in node.children().filter(|n| n.is_element()) {
        match child.tag_name().name() {
            "cocos" => {
                let from_cocos = required_attr(&child, "cocos", "from")?.to_string();
                let to_cocos = required_attr(&child, "cocos", "to")?.to_string();
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
        assert_eq!(map.left.dd, "3.39.0");
        assert_eq!(map.left.cocos, "11");
        assert_eq!(map.right.dd, "4.1.1");
        assert_eq!(map.right.cocos, "17");
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

        // merged: this DD3 alias feeds a merged destination, not an identical one.
        assert_eq!(
            map.resolve("time_slice/constraints/j_tor", Direction::Forward),
            None
        );

        // moved: this DD3 path relocated under `boundary`, not kept in place.
        assert_eq!(
            map.resolve("time_slice/boundary_separatrix/gap", Direction::Forward),
            None
        );

        // split: this DD3 path feeds two DD4 destinations, not one identical one.
        assert_eq!(
            map.resolve("time_slice/global_quantities/psi_axis", Direction::Forward),
            None
        );

        // right_only: this DD4-only path has no DD3 spelling to fall back to.
        assert_eq!(
            map.resolve("time_slice/contour_tree", Direction::Reverse),
            None
        );
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
                from_cocos: "11".to_string(),
                to_cocos: "17".to_string(),
            }
        );

        let reverse = map
            .resolve("time_slice/boundary/psi", Direction::Reverse)
            .expect("identical path must resolve in reverse too");
        assert_eq!(reverse.match_kind, MatchKind::Default);
        assert_eq!(
            reverse.value_transformation,
            ValueTransformation::SignFlip {
                from_cocos: "11".to_string(),
                to_cocos: "17".to_string(),
            }
        );
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
}
