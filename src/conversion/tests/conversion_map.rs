use super::*;

const APPROVED_ARTIFACT: &str = include_str!("../../../docs/3.39.0--4.1.1.xml");
const LEFT_INVENTORY_339: &str = include_str!("../../../docs/inventory/equilibrium-3.39.0.txt");
const RIGHT_INVENTORY_411: &str = include_str!("../../../docs/inventory/equilibrium-4.1.1.txt");

fn parse_inventory(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn resolved_path(explanation: &RuleExplanation) -> &str {
    match &explanation.outcome {
        Outcome::Path { resolved_path, .. } => resolved_path,
        other => panic!("expected a translated path, got {other:?}"),
    }
}

fn value_transformation(explanation: &RuleExplanation) -> &ValueTransformation {
    match &explanation.outcome {
        Outcome::Path {
            value_transformation,
            ..
        } => value_transformation,
        other => panic!("expected a translated path, got {other:?}"),
    }
}

fn candidates(explanation: &RuleExplanation) -> &[CandidatePath] {
    match &explanation.outcome {
        Outcome::Path { candidates, .. } => candidates,
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
fn rejects_glob_merged_and_split_rules_with_unrenderable_candidate_paths() {
    for xml in [
        r#"
                <ids-map ids="equilibrium" format-version="1">
                  <side id="left" dd="3.39.0" cocos="11"/>
                  <side id="right" dd="4.1.1" cocos="17"/>
                  <rules>
                    <rule id="merge" rel="merged" glob="yes" right="right/*">
                      <from left="left/*/*" precedence="1"/>
                      <fidelity forward="exact" reverse="exact"/>
                    </rule>
                  </rules>
                </ids-map>
            "#,
        r#"
                <ids-map ids="equilibrium" format-version="1">
                  <side id="left" dd="3.39.0" cocos="11"/>
                  <side id="right" dd="4.1.1" cocos="17"/>
                  <rules>
                    <rule id="split" rel="split" glob="yes" left="left/*">
                      <from right="right/*/*" precedence="1"/>
                      <fidelity forward="exact" reverse="exact"/>
                    </rule>
                  </rules>
                </ids-map>
            "#,
    ] {
        let err = ConversionMap::load(xml).unwrap_err();
        assert!(matches!(err, LoadError::InvalidRuleShape { .. }));
    }
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
    assert_eq!(resolved_path(&explanation), "vacuum_toroidal_field/b0");
    assert_eq!(explanation.fidelity, Fidelity::Exact);
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
    assert_eq!(
        resolved_path(&forward),
        "time_slice/global_quantities/beta_tor_norm"
    );
    assert_eq!(forward.fidelity, Fidelity::Exact);
    assert_eq!(*value_transformation(&forward), ValueTransformation::None);

    let reverse = map
        .resolve(
            "time_slice/global_quantities/beta_tor_norm",
            Direction::Reverse,
        )
        .expect("known DD4 name must resolve back to its DD3 spelling");
    assert_eq!(reverse.match_kind, MatchKind::Explicit);
    assert_eq!(reverse.rule_id.as_deref(), Some("rename-beta-normal"));
    assert_eq!(
        resolved_path(&reverse),
        "time_slice/global_quantities/beta_normal"
    );
    assert_eq!(reverse.fidelity, Fidelity::Exact);
    assert_eq!(*value_transformation(&reverse), ValueTransformation::None);
}

#[test]
fn side_only_rules_do_not_default_to_identity() {
    let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");

    let left_only = map
        .resolve("time_slice/boundary/lcfs", Direction::Forward)
        .expect("a left-only rule claims this path");
    assert_eq!(left_only.outcome, Outcome::NoSource);

    let right_only = map
        .resolve("time_slice/contour_tree", Direction::Reverse)
        .expect("a right-only rule claims this path");
    assert_eq!(right_only.outcome, Outcome::NoSource);
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

    let candidate_paths: Vec<(&str, u32)> = candidates(&explanation)
        .iter()
        .map(|c| (c.path.as_str(), c.precedence))
        .collect();
    assert_eq!(
        candidate_paths,
        vec![
            ("time_slice/profiles_2d/b_field_phi", 1),
            ("time_slice/profiles_2d/b_field_tor", 2),
            ("time_slice/profiles_2d/b_tor", 3),
        ]
    );
    // The precedence-first candidate is surfaced as the primary result.
    assert_eq!(
        resolved_path(&explanation),
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
    let map = ConversionMap::load(xml).expect("declared precedence need not follow document order");

    let explanation = map
        .resolve("right/canonical", Direction::Reverse)
        .expect("merged rule must resolve in reverse");
    let paths: Vec<&str> = candidates(&explanation)
        .iter()
        .map(|c| c.path.as_str())
        .collect();
    assert_eq!(paths, vec!["left/first", "left/second"]);
}

#[test]
fn a_merged_rule_declared_unmappable_refuses_instead_of_planning_candidates() {
    let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="fold" rel="merged" right="b">
                  <from left="a1" precedence="1"/>
                  <from left="a2" precedence="2"/>
                  <fidelity forward="unmappable" reverse="unmappable"/>
                </rule>
              </rules>
            </ids-map>
        "#;
    let map = ConversionMap::load(xml).expect("map loads");

    // Forward, the declared alias must not resolve to the canonical path.
    let forward = map
        .resolve("a1", Direction::Forward)
        .expect("a declared alias matches its merged rule");
    assert_eq!(forward.match_kind, MatchKind::Explicit);
    assert_eq!(forward.rule_id.as_deref(), Some("fold"));
    assert_eq!(forward.fidelity, Fidelity::Unmappable);
    assert_eq!(
        forward.outcome,
        Outcome::Refusal(RefusalReason::Unmappable),
        "a merged rule declared unmappable must refuse, not resolve"
    );

    // Reverse is the direction that would otherwise hand the read path a
    // candidate plan to execute against IMAS-Core.
    let reverse = map
        .resolve("b", Direction::Reverse)
        .expect("the canonical path matches its merged rule");
    assert_eq!(reverse.fidelity, Fidelity::Unmappable);
    assert_eq!(
        reverse.outcome,
        Outcome::Refusal(RefusalReason::Unmappable),
        "a merged rule declared unmappable must not produce a candidate read plan"
    );
}

#[test]
fn a_split_rule_declared_unmappable_refuses_instead_of_planning_candidates() {
    let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="fan" rel="split" left="a">
                  <from right="b1" precedence="1"/>
                  <from right="b2" precedence="2"/>
                  <fidelity forward="unmappable" reverse="unmappable"/>
                </rule>
              </rules>
            </ids-map>
        "#;
    let map = ConversionMap::load(xml).expect("map loads");

    // Forward is the candidate-plan direction for a split rule.
    let forward = map
        .resolve("a", Direction::Forward)
        .expect("the single source matches its split rule");
    assert_eq!(forward.match_kind, MatchKind::Explicit);
    assert_eq!(forward.rule_id.as_deref(), Some("fan"));
    assert_eq!(forward.fidelity, Fidelity::Unmappable);
    assert_eq!(
        forward.outcome,
        Outcome::Refusal(RefusalReason::Unmappable),
        "a split rule declared unmappable must not produce a candidate read plan"
    );

    let reverse = map
        .resolve("b1", Direction::Reverse)
        .expect("a declared destination matches its split rule");
    assert_eq!(reverse.fidelity, Fidelity::Unmappable);
    assert_eq!(reverse.outcome, Outcome::Refusal(RefusalReason::Unmappable));
}

#[test]
fn a_retype_refuses_on_shape_even_when_it_declares_unmappable() {
    let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="reshape" rel="retyped" left="a" right="a">
                  <fidelity forward="unmappable" reverse="unmappable"/>
                </rule>
              </rules>
            </ids-map>
        "#;
    let map = ConversionMap::load(xml).expect("map loads");

    // Pins the precedence between the two pre-resolution refusals: the
    // shape reason wins, so a retype never reports the generic one.
    let explanation = map.resolve("a", Direction::Forward).expect("rule matches");
    assert_eq!(
        explanation.outcome,
        Outcome::Refusal(RefusalReason::UnservableRetype)
    );
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
    assert_eq!(resolved_path(&explanation), "time_slice/constraints/j_phi");
    assert_eq!(explanation.precedence, Some(2));
    assert_eq!(explanation.fidelity, Fidelity::Lossy);
    assert!(candidates(&explanation).is_empty());
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
        resolved_path(&explanation),
        "time_slice/global_quantities/psi_axis"
    );

    let candidate_paths: Vec<(&str, u32)> = candidates(&explanation)
        .iter()
        .map(|c| (c.path.as_str(), c.precedence))
        .collect();
    assert_eq!(
        candidate_paths,
        vec![
            ("time_slice/global_quantities/psi_axis", 1),
            ("time_slice/global_quantities/psi_magnetic_axis", 2),
        ]
    );
    // Both destinations are separately declared `<flip>` targets; each
    // candidate must carry its own value transformation rather than one
    // shared for the whole rule.
    for candidate in candidates(&explanation) {
        assert_eq!(
            candidate.value_transformation,
            ValueTransformation::SignFlip {
                from_cocos: CocosConvention("17".to_string()),
                to_cocos: CocosConvention("11".to_string()),
                direction: TransformationDirection::ToHli,
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
        resolved_path(&explanation),
        "time_slice/global_quantities/psi_axis"
    );
    assert_eq!(explanation.precedence, Some(2));
    assert_eq!(explanation.fidelity, Fidelity::Exact);
    assert_eq!(
        *value_transformation(&explanation),
        ValueTransformation::SignFlip {
            from_cocos: CocosConvention("11".to_string()),
            to_cocos: CocosConvention("17".to_string()),
            direction: TransformationDirection::ToHli,
        }
    );
    assert!(candidates(&explanation).is_empty());
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
        *value_transformation(&forward),
        ValueTransformation::SignFlip {
            from_cocos: CocosConvention("17".to_string()),
            to_cocos: CocosConvention("11".to_string()),
            direction: TransformationDirection::ToHli,
        }
    );

    let reverse = map
        .resolve("time_slice/boundary/psi", Direction::Reverse)
        .expect("identical path must resolve in reverse too");
    assert_eq!(reverse.match_kind, MatchKind::Default);
    assert_eq!(
        *value_transformation(&reverse),
        ValueTransformation::SignFlip {
            from_cocos: CocosConvention("11".to_string()),
            to_cocos: CocosConvention("17".to_string()),
            direction: TransformationDirection::ToHli,
        }
    );
}

#[test]
fn a_read_transformation_inverts_to_the_stored_write_direction() {
    let read = ValueTransformation::SignFlip {
        from_cocos: CocosConvention("17".to_string()),
        to_cocos: CocosConvention("11".to_string()),
        direction: TransformationDirection::ToHli,
    };

    assert_eq!(
        read.inverse(),
        Some(ValueTransformation::SignFlip {
            from_cocos: CocosConvention("11".to_string()),
            to_cocos: CocosConvention("17".to_string()),
            direction: TransformationDirection::ToStored,
        })
    );
}

#[test]
fn same_cocos_flip_plan_normalizes_to_identity() {
    let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="11"/>
              <default rel="identical"/>
              <transforms>
                <cocos from="11" to="11">
                  <flip path="time_slice/boundary/psi"/>
                </cocos>
              </transforms>
            </ids-map>
        "#;
    let map = ConversionMap::load(xml).expect("same-convention map must load");

    for direction in [Direction::Forward, Direction::Reverse] {
        let explanation = map
            .resolve("time_slice/boundary/psi", direction)
            .expect("same-convention flip path must resolve");
        assert_eq!(
            *value_transformation(&explanation),
            ValueTransformation::None
        );
    }
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
    // `time_slice/boundary/lcfs`. A rule kind this issue does not resolve
    // still must be recognised as claiming every path nested under its
    // anchor, or such a path would incorrectly fall through to the
    // document-level identity default instead of correctly declining to
    // resolve.
    let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");
    let explanation = map
        .resolve("time_slice/boundary/lcfs/r", Direction::Forward)
        .expect("the subtree rule claims this descendant");
    assert_eq!(explanation.outcome, Outcome::NoSource);
}

// ADR 0017 decision 4 / CONTEXT.md's "escaping rule": a subtree delete of
// `path` is trivial iff every rule nested at or under it keeps its stored
// target(s) at or under `path`'s own resolved stored spelling. These tests
// exercise `ConversionMap::subtree_delete_is_trivial` directly, against
// small purpose-built fixtures rather than the approved artifact, so every
// rule kind's escape behaviour is covered — not only `moved`, which is the
// only kind that happens to escape in the shipped artifact.

#[test]
fn the_approved_artifacts_only_escaping_rules_are_its_moved_rules() {
    let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");

    // Forward (a DD3 HLI): the three `moved` rules anchored under
    // `boundary_separatrix` all target `time_slice/boundary/...`, so a
    // delete of the whole `boundary_separatrix` subtree would leave that
    // data behind, unreached, at a path outside it.
    assert!(!map.subtree_delete_is_trivial(
        "time_slice/boundary_separatrix",
        "time_slice/boundary_separatrix",
        Direction::Forward,
    ));
    // Reverse (a DD4 HLI): the mirror image — the same three rules'
    // right-side selectors sit under `boundary`, targeting
    // `boundary_separatrix`.
    assert!(!map.subtree_delete_is_trivial(
        "time_slice/boundary",
        "time_slice/boundary",
        Direction::Reverse,
    ));

    // Forward `boundary` and both directions of `constraints` and the
    // `time_slice` root itself carry no rule whose target crosses back out,
    // so all four remain trivial — the ADR's own allowed examples.
    assert!(map.subtree_delete_is_trivial(
        "time_slice/boundary",
        "time_slice/boundary",
        Direction::Forward,
    ));
    assert!(map.subtree_delete_is_trivial(
        "time_slice/constraints",
        "time_slice/constraints",
        Direction::Forward,
    ));
    assert!(map.subtree_delete_is_trivial(
        "time_slice/constraints",
        "time_slice/constraints",
        Direction::Reverse,
    ));
    assert!(map.subtree_delete_is_trivial("time_slice", "time_slice", Direction::Forward));
    assert!(map.subtree_delete_is_trivial("time_slice", "time_slice", Direction::Reverse));
}

#[test]
fn a_renamed_rule_escapes_only_when_it_crosses_the_requested_boundary() {
    let non_crossing = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="11"/>
              <rules>
                <rule id="rename-within" rel="renamed"
                      left="container/old_name" right="container/new_name">
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
    let map = ConversionMap::load(non_crossing).expect("fixture artifact must load");
    assert!(map.subtree_delete_is_trivial("container", "container", Direction::Forward));
    assert!(map.subtree_delete_is_trivial("container", "container", Direction::Reverse));

    let crossing = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="11"/>
              <rules>
                <rule id="rename-cross" rel="renamed"
                      left="container/child" right="elsewhere/child">
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
    let map = ConversionMap::load(crossing).expect("fixture artifact must load");
    assert!(!map.subtree_delete_is_trivial("container", "container", Direction::Forward));
    assert!(!map.subtree_delete_is_trivial("elsewhere", "elsewhere", Direction::Reverse));
}

#[test]
fn a_moved_rule_always_escapes_its_own_source_subtree() {
    let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="11"/>
              <rules>
                <rule id="move-a" rel="moved"
                      left="container/sub/a" right="other/a" subtree="yes">
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
    let map = ConversionMap::load(xml).expect("fixture artifact must load");
    assert!(!map.subtree_delete_is_trivial("container/sub", "container/sub", Direction::Forward,));
    assert!(!map.subtree_delete_is_trivial("container", "container", Direction::Forward));
    assert!(!map.subtree_delete_is_trivial("other", "other", Direction::Reverse));
}

#[test]
fn a_merged_rule_escapes_only_when_a_candidate_lands_outside_the_subtree() {
    let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="11"/>
              <rules>
                <rule id="fold" rel="merged" right="container/canonical">
                  <from left="container/a" precedence="1"/>
                  <from left="elsewhere/b" precedence="2"/>
                  <fidelity forward="lossy" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
    let map = ConversionMap::load(xml).expect("fixture artifact must load");

    // Reverse: the rule's single source role is `right`, so a delete of
    // `container` sees it and must check every `<from>` candidate —
    // `elsewhere/b` lands outside `container`.
    assert!(!map.subtree_delete_is_trivial("container", "container", Direction::Reverse));
    // Forward: `elsewhere/b` is itself one of the source entries, so a
    // delete of `elsewhere` sees this rule too, and its one target
    // (`container/canonical`) lands outside `elsewhere`.
    assert!(!map.subtree_delete_is_trivial("elsewhere", "elsewhere", Direction::Forward));
    // A delete of `container` forward only ever sees the `container/a`
    // source entry, whose target is the in-subtree canonical path.
    assert!(map.subtree_delete_is_trivial("container", "container", Direction::Forward));
}

#[test]
fn a_split_rule_escapes_only_when_a_destination_lands_outside_the_subtree() {
    let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="11"/>
              <rules>
                <rule id="split" rel="split" left="container/canonical">
                  <from right="container/a" precedence="1"/>
                  <from right="elsewhere/b" precedence="2"/>
                  <fidelity forward="exact" reverse="lossy"/>
                </rule>
              </rules>
            </ids-map>
        "#;
    let map = ConversionMap::load(xml).expect("fixture artifact must load");

    // Forward: the rule's single source role is `left`, so a delete of
    // `container` sees it and must check every `<from>` destination —
    // `elsewhere/b` lands outside `container`.
    assert!(!map.subtree_delete_is_trivial("container", "container", Direction::Forward));
    // Reverse: `elsewhere/b` is itself one of the source entries, so a
    // delete of `elsewhere` sees this rule too, and its one target
    // (`container/canonical`) lands outside `elsewhere`.
    assert!(!map.subtree_delete_is_trivial("elsewhere", "elsewhere", Direction::Reverse));
    assert!(map.subtree_delete_is_trivial("container", "container", Direction::Reverse));
}

#[test]
fn left_only_and_right_only_rules_never_escape() {
    let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="11"/>
              <rules>
                <rule id="drop-a" rel="left_only" left="container/gone">
                  <fidelity forward="lossy" reverse="unmappable"/>
                </rule>
                <rule id="new-b" rel="right_only" right="container/new">
                  <fidelity forward="unmappable" reverse="lossy"/>
                </rule>
              </rules>
            </ids-map>
        "#;
    let map = ConversionMap::load(xml).expect("fixture artifact must load");

    // `left_only`/`right_only` declare no stored counterpart at all, so a
    // rule nested under the requested subtree — with nothing to escape
    // to — never breaks triviality.
    assert!(map.subtree_delete_is_trivial("container", "container", Direction::Forward));
    assert!(map.subtree_delete_is_trivial("container", "container", Direction::Reverse));
}

#[test]
fn a_retyped_rule_nested_under_a_subtree_never_escapes() {
    // A `retyped` rule's own left and right selectors are the identical
    // spelling (only the container shape changes), so it can never be an
    // escaping rule: its one target is always the same location as its
    // source, which is by construction inside whatever subtree the source
    // is inside.
    let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="11"/>
              <rules>
                <rule id="retype" rel="retyped"
                      left="container/shape" right="container/shape" subtree="yes">
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
    let map = ConversionMap::load(xml).expect("fixture artifact must load");
    assert!(map.subtree_delete_is_trivial("container", "container", Direction::Forward));
    assert!(map.subtree_delete_is_trivial("container", "container", Direction::Reverse));
}

#[test]
fn a_subtree_with_no_nested_rules_is_vacuously_trivial() {
    let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="11"/>
              <rules>
                <rule id="rename-elsewhere" rel="renamed"
                      left="unrelated/old_name" right="somewhere_else/new_name">
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
    let map = ConversionMap::load(xml).expect("fixture artifact must load");
    assert!(map.subtree_delete_is_trivial(
        "container/untouched",
        "container/untouched",
        Direction::Forward,
    ));
}

#[test]
fn refusal_and_moved_outcomes_retain_the_rule_explanation() {
    let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");

    let retype = map
        .resolve("grids_ggd/grid/space/coordinates_type", Direction::Forward)
        .expect("the retype rule claims this path");
    assert_eq!(retype.rule_id.as_deref(), Some("retype-coordinates-type"));
    assert_eq!(retype.fidelity, Fidelity::Exact);
    assert_eq!(
        retype.outcome,
        Outcome::Refusal(RefusalReason::UnservableRetype)
    );

    let moved = map
        .resolve(
            "time_slice/boundary_separatrix/closest_wall_point/r",
            Direction::Forward,
        )
        .expect("the moved rule claims this path");
    assert_eq!(moved.rule_id.as_deref(), Some("move-closest-wall-point"));
    assert_eq!(moved.fidelity, Fidelity::Exact);
    assert_eq!(
        resolved_path(&moved),
        "time_slice/boundary/closest_wall_point/r"
    );

    let unit_redefinition = map
        .resolve(
            "time_slice/constraints/strike_point/chi_squared_r",
            Direction::Forward,
        )
        .expect("the identity default resolves this path");
    assert_eq!(unit_redefinition.match_kind, MatchKind::Default);
    assert_eq!(unit_redefinition.fidelity, Fidelity::Unmappable);
    assert_eq!(
        unit_redefinition.outcome,
        Outcome::Refusal(RefusalReason::UnitRedefinition)
    );
}

#[test]
fn overlapping_redefine_globs_invalidate_the_map() {
    let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <transforms>
                <redefine glob="time_slice/*/chi_squared_r" left-units="m" right-units="m^-2">
                  <fidelity forward="unmappable" reverse="unmappable"/>
                </redefine>
                <redefine glob="time_slice/constraints/*" left-units="m" right-units="m^-2">
                  <fidelity forward="lossy" reverse="lossy"/>
                </redefine>
              </transforms>
            </ids-map>
        "#;
    assert_eq!(
        ConversionMap::load(xml).unwrap_err(),
        LoadError::OverlappingRedefineSelectors {
            first: "time_slice/*/chi_squared_r".to_string(),
            second: "time_slice/constraints/*".to_string(),
        }
    );
}

#[test]
fn unclaimed_inventory_path_fails_completeness_check() {
    let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
            </ids-map>
        "#;
    let map = ConversionMap::load(xml).expect("a map with no default and no rules loads");
    let left_inventory = vec!["orphan/path".to_string()];
    let right_inventory: Vec<String> = vec![];

    let violations = map
        .check_completeness(&left_inventory, &right_inventory)
        .expect_err("a path with no rule and no default must fail completeness");
    assert!(
        violations.contains(&CompletenessViolation::UnclaimedInventoryPath {
            side: InventorySide::Left,
            path: "orphan/path".to_string(),
        })
    );
}

#[test]
fn default_match_assuming_a_missing_counterpart_fails_completeness_check() {
    let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <default rel="identical"/>
            </ids-map>
        "#;
    let map = ConversionMap::load(xml).expect("map loads");
    // "a" is not actually present on the right, so the identity default
    // silently assumes a path that does not exist there.
    let left_inventory = vec!["a".to_string()];
    let right_inventory: Vec<String> = vec![];

    let violations = map
        .check_completeness(&left_inventory, &right_inventory)
        .expect_err("the default cannot silently assume a nonexistent counterpart");
    assert!(
        violations.contains(&CompletenessViolation::DefaultAssumesMissingCounterpart {
            side: InventorySide::Left,
            path: "a".to_string(),
        })
    );
}

#[test]
fn identical_path_on_both_sides_satisfies_the_default_match() {
    let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <default rel="identical"/>
            </ids-map>
        "#;
    let map = ConversionMap::load(xml).expect("map loads");
    let inventory = vec!["a".to_string()];
    assert_eq!(map.check_completeness(&inventory, &inventory), Ok(()));
}

#[test]
fn explicit_rule_claims_a_path_absent_from_the_other_side() {
    let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="drop-a" rel="left_only" left="a">
                  <fidelity forward="lossy" reverse="unmappable"/>
                </rule>
              </rules>
            </ids-map>
        "#;
    let map = ConversionMap::load(xml).expect("map loads");
    let left_inventory = vec!["a".to_string()];
    let right_inventory: Vec<String> = vec![];
    assert_eq!(
        map.check_completeness(&left_inventory, &right_inventory),
        Ok(())
    );
}

#[test]
fn rule_selector_not_backed_by_inventory_fails_completeness_check() {
    let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="drop-invented" rel="left_only" left="totally/invented/path">
                  <fidelity forward="lossy" reverse="unmappable"/>
                </rule>
              </rules>
            </ids-map>
        "#;
    let map = ConversionMap::load(xml).expect("map loads");
    // No real path anywhere is named or nested under this rule's claim.
    let left_inventory: Vec<String> = vec![];
    let right_inventory: Vec<String> = vec![];

    let violations = map
        .check_completeness(&left_inventory, &right_inventory)
        .expect_err("a rule with no basis in the raw inventory must fail completeness");
    assert!(
        violations.contains(&CompletenessViolation::RuleSelectorNotBackedByInventory {
            rule_id: "drop-invented".to_string(),
            side: InventorySide::Left,
            pattern: "totally/invented/path".to_string(),
        })
    );
}

#[test]
fn merged_rule_from_candidate_absent_from_inventory_is_tolerated() {
    let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="fold" rel="merged" right="modern">
                  <from left="modern" precedence="1"/>
                  <from left="legacy" precedence="2"/>
                  <fidelity forward="lossy" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
    let map = ConversionMap::load(xml).expect("map loads");
    // "modern" does not exist yet at this pinned left-side snapshot,
    // mirroring the real artifact's fold-constraints-j: its precedence-1
    // alias post-dates 3.39.0, and the proof must tolerate that rather
    // than reject the rule (issue #50's "paths introduced on a rule side
    // that do not occur in the corresponding raw inventory").
    let left_inventory = vec!["legacy".to_string()];
    let right_inventory = vec!["modern".to_string()];

    assert_eq!(
        map.check_completeness(&left_inventory, &right_inventory),
        Ok(())
    );
}

#[test]
fn subtree_rule_anchor_is_backed_by_a_nested_descendant_even_when_absent_itself() {
    let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="drop-group" rel="left_only" left="a/group" subtree="yes">
                  <fidelity forward="lossy" reverse="unmappable"/>
                </rule>
              </rules>
            </ids-map>
        "#;
    let map = ConversionMap::load(xml).expect("map loads");
    // "a/group" itself is a container never listed as its own leaf --
    // only a descendant path is.
    let left_inventory = vec!["a/group/leaf".to_string()];
    let right_inventory: Vec<String> = vec![];

    assert_eq!(
        map.check_completeness(&left_inventory, &right_inventory),
        Ok(())
    );
}

#[test]
fn retyped_rule_anchor_absent_from_inventory_is_backed_by_its_shape_derived_child() {
    // Story 40 (#43) names this scenario explicitly: a retyped-style
    // rule's container-level anchor may not itself be a raw DD leaf once
    // its shape changes -- this mirrors the real artifact's
    // retype-coordinates-type rule, whose anchor is confirmed via
    // imas-dd's version history to change from INT_1D to STRUCT_ARRAY at
    // DD4's 4.0.0 boundary, so a leaf-only inventory can plausibly list
    // only the shape's own child leaf ("index"), not the container
    // itself. The same Subtree-backing tolerance that already lets
    // left_only/right_only/moved subtree rules claim a container without
    // being a raw leaf themselves covers this case too -- no
    // retyped-specific exemption is needed (ADR 0013).
    let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="retype-coordinates-type" rel="retyped"
                      left="grids_ggd/grid/space/coordinates_type"
                      right="grids_ggd/grid/space/coordinates_type"
                      subtree="yes">
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
    let map = ConversionMap::load(xml).expect("retyped rule is structurally valid");
    let left_inventory = vec!["grids_ggd/grid/space/coordinates_type".to_string()];
    // Deliberately omit the bare anchor on the right -- only its
    // shape-derived child is present, exactly like a leaf-only listing.
    let right_inventory = vec!["grids_ggd/grid/space/coordinates_type/index".to_string()];

    assert_eq!(
        map.check_completeness(&left_inventory, &right_inventory),
        Ok(())
    );
}

/// Pins which refusal reasons and selector stages the approved artifact can
/// actually reach, by sweeping both real inventories in both directions.
/// `check_completeness`'s doc comment already sets the precedent for
/// pinning a reachability fact rather than leaving a reader to derive it.
///
/// Two of these facts are counter-intuitive and were both mis-read during
/// review, which is why they are asserted rather than commented:
///
/// - `RefusalReason::Unmappable` is unreachable. The artifact declares
///   `unmappable` thirty-six times, but every one sits on a `left_only`
///   rule's `reverse` or a `right_only` rule's `forward` — the direction
///   that rule can never be selected in, since it has no path indexed on
///   that side (see [`Rel::LeftOnly`]). So the shipped artifact's refusals
///   are only ever the shape one and the unit one, and this variant's real
///   coverage is the synthetic-artifact tests, not this artifact.
/// - The glob selector stage is unreachable too: no rule in the artifact
///   uses a glob selector. The four `<redefine glob="...">` entries do,
///   but they are matched by `redefine_for` against a resolved right-side
///   path, not by `best_match`'s stage ladder.
///
/// Neither is a defect, and neither should be "fixed" by editing the
/// artifact. They bound what a green suite proves: if a future artifact
/// makes either reachable, this test fails and the reader is told to go
/// add real coverage for the mechanism rather than trusting the synthetic
/// tests alone (ADR 0011's "silence is earned by mechanism coverage").
#[test]
fn the_approved_artifact_reaches_only_its_shape_and_unit_refusals() {
    let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");
    let left_inventory = parse_inventory(LEFT_INVENTORY_339);
    let right_inventory = parse_inventory(RIGHT_INVENTORY_411);

    for (label, inventory, direction, expected_retypes) in [
        ("forward", &left_inventory, Direction::Forward, 1),
        // The 4.1.1 inventory lists the retype's container and the
        // `index` child its shape change introduced, so both refuse.
        ("reverse", &right_inventory, Direction::Reverse, 2),
    ] {
        let mut retypes = 0;
        let mut unit_redefinitions = 0;
        let mut unmappables = 0;
        let mut globs = 0;

        for path in inventory {
            let Some(explanation) = map.resolve(path, direction) else {
                continue;
            };
            if explanation.selector_stage == Some(SelectorStage::Glob) {
                globs += 1;
            }
            match explanation.outcome {
                Outcome::Refusal(RefusalReason::UnservableRetype) => retypes += 1,
                Outcome::Refusal(RefusalReason::UnitRedefinition) => unit_redefinitions += 1,
                Outcome::Refusal(RefusalReason::Unmappable) => unmappables += 1,
                _ => {}
            }
        }

        assert_eq!(retypes, expected_retypes, "{label} retype refusals");
        assert_eq!(unit_redefinitions, 4, "{label} unit-redefinition refusals");
        assert_eq!(
            unmappables, 0,
            "{label}: RefusalReason::Unmappable became reachable from the approved \
                 artifact -- its only coverage was synthetic, so add a real-artifact test"
        );
        assert_eq!(
            globs, 0,
            "{label}: the glob selector stage became reachable from the approved \
                 artifact -- it had no real-artifact coverage, so add some"
        );
    }
}

/// The structural reason `RefusalReason::Unmappable` is unreachable above,
/// shown on one concrete rule rather than argued in prose.
///
/// `drop-b-flux-pol-norm` is `left_only` over a DD3-only path and declares
/// `forward="lossy" reverse="unmappable"`. Forward, it is selected and
/// yields no source. Reverse, it cannot be selected at all — so the
/// identity default answers instead, and the `unmappable` it declares is
/// never consulted by anything.
#[test]
fn a_left_only_rules_reverse_fidelity_is_never_consulted() {
    let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");
    let path = "time_slice/boundary/b_flux_pol_norm";

    let forward = map
        .resolve(path, Direction::Forward)
        .expect("the left_only rule claims its own side");
    assert_eq!(forward.match_kind, MatchKind::Explicit);
    assert_eq!(forward.rule_id.as_deref(), Some("drop-b-flux-pol-norm"));
    assert_eq!(forward.rel, Some(Rel::LeftOnly));
    assert_eq!(forward.fidelity, Fidelity::Lossy);
    assert_eq!(forward.outcome, Outcome::NoSource);

    let reverse = map
        .resolve(path, Direction::Reverse)
        .expect("the identity default still answers");
    assert_eq!(
        reverse.match_kind,
        MatchKind::Default,
        "a left_only rule must not be selectable in reverse"
    );
    assert_eq!(reverse.rule_id, None);
    assert_ne!(
        reverse.fidelity,
        Fidelity::Unmappable,
        "the rule's declared reverse=unmappable is documentation, not a resolver outcome"
    );
}

#[test]
fn approved_artifact_completeness_holds_against_real_dd_inventories() {
    let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");
    let left_inventory = parse_inventory(LEFT_INVENTORY_339);
    let right_inventory = parse_inventory(RIGHT_INVENTORY_411);

    let result = map.check_completeness(&left_inventory, &right_inventory);
    assert_eq!(result, Ok(()), "completeness violations: {result:?}");
}

/// The shim reads this one path at every open to discover the stored DD
/// version (ADR 0007), so the completeness proof has to claim it. The
/// imas-dd path sets the inventories derive from exclude
/// `ids_properties/**` wholesale, so it is in both files by hand — and a
/// hand-added entry that silently dropped out again would take the shim's
/// own read path out of the proof without failing anything else.
#[test]
fn the_version_stamp_path_the_shim_itself_reads_is_inside_the_proof() {
    const STAMP: &str = "ids_properties/version_put/data_dictionary";
    let left_inventory = parse_inventory(LEFT_INVENTORY_339);
    let right_inventory = parse_inventory(RIGHT_INVENTORY_411);
    assert!(
        left_inventory.iter().any(|path| path == STAMP),
        "the 3.39.0 inventory must list the stamp path the shim reads"
    );
    assert!(
        right_inventory.iter().any(|path| path == STAMP),
        "the 4.1.1 inventory must list the stamp path the shim reads"
    );

    let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");
    for direction in [Direction::Forward, Direction::Reverse] {
        let explanation = map
            .resolve(STAMP, direction)
            .expect("the stamp path must be claimed in both directions");
        assert_eq!(resolved_path(&explanation), STAMP);
    }
}

/// Issue #50 AC1 reads "every DD path from both … inventories is claimed
/// by a rule", and the approved artifact's document-level
/// `<default rel="identical"/>` means no path is ever *unclaimed* — so
/// this is the violation that can never fire for the shipped artifact,
/// pinned here so the next reader does not mistake the gate for more
/// than it is. `default_assumes_missing_counterpart_*` below is the
/// assertion that actually carries AC1's weight.
#[test]
fn the_approved_artifact_can_never_report_an_unclaimed_path() {
    let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");
    let left_inventory = vec!["invented/path/no/rule/mentions".to_string()];
    let right_inventory: Vec<String> = vec![];

    let violations = map
        .check_completeness(&left_inventory, &right_inventory)
        .expect_err("an invented path with no counterpart must still fail");
    assert!(
        !violations.iter().any(|violation| matches!(
            violation,
            CompletenessViolation::UnclaimedInventoryPath { .. }
        )),
        "the identity default claims every path, so this violation is unreachable: \
             {violations:?}"
    );
    assert!(
        violations.contains(&CompletenessViolation::DefaultAssumesMissingCounterpart {
            side: InventorySide::Left,
            path: "invented/path/no/rule/mentions".to_string(),
        }),
        "{violations:?}"
    );
}

/// The same reachability fact from the other side: the load-bearing
/// violation fires against the *shipped* map, not only against a
/// hand-built toy one, when a real inventory path loses its counterpart.
#[test]
fn a_side_only_rule_contradicted_by_the_other_inventory_fails_completeness_check() {
    let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");
    let left_inventory = parse_inventory(LEFT_INVENTORY_339);
    // drop-timeslice-ggd-grid declares the whole time_slice/ggd/grid
    // subtree gone in DD4, and the imas-dd path sets agree. The shipped
    // 4.1.1 inventory listed 23 of its paths anyway; because they were
    // then present on both sides, DefaultAssumesMissingCounterpart stayed
    // silent, the identity default claimed them, and the reverse coverage
    // figure counted all 23 as supported. Reintroducing one is enough to
    // reproduce that, and it must now be rejected.
    let reintroduced = "time_slice/ggd/grid/path";
    assert!(left_inventory.iter().any(|path| path == reintroduced));
    let mut right_inventory = parse_inventory(RIGHT_INVENTORY_411);
    assert!(!right_inventory.iter().any(|path| path == reintroduced));
    right_inventory.push(reintroduced.to_string());

    let violations = map
        .check_completeness(&left_inventory, &right_inventory)
        .expect_err("a left_only rule's own path must not exist on the right");
    assert!(
        violations.contains(
            &CompletenessViolation::SideOnlyRuleContradictedByInventory {
                rule_id: "drop-timeslice-ggd-grid".to_string(),
                side: InventorySide::Right,
                pattern: "time_slice/ggd/grid".to_string(),
                path: reintroduced.to_string(),
            }
        ),
        "{violations:?}"
    );
}

#[test]
fn a_right_only_rule_contradicted_by_the_left_inventory_fails_completeness_check() {
    let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <default rel="identical"/>
              <rules>
                <rule id="new-b" rel="right_only" right="a/b">
                  <fidelity forward="unmappable" reverse="lossy"/>
                </rule>
              </rules>
            </ids-map>
        "#;
    let map = ConversionMap::load(xml).expect("map loads");
    // The rule says a/b is new on the right, so the left inventory
    // listing it is the same contradiction in the mirror direction.
    let left_inventory = vec!["a/b".to_string()];
    let right_inventory = vec!["a/b".to_string()];

    let violations = map
        .check_completeness(&left_inventory, &right_inventory)
        .expect_err("a right_only rule's own path must not exist on the left");
    assert!(
        violations.contains(
            &CompletenessViolation::SideOnlyRuleContradictedByInventory {
                rule_id: "new-b".to_string(),
                side: InventorySide::Left,
                pattern: "a/b".to_string(),
                path: "a/b".to_string(),
            }
        ),
        "{violations:?}"
    );
}

#[test]
fn the_approved_artifact_rejects_a_real_path_whose_counterpart_disappears() {
    let map = ConversionMap::load(APPROVED_ARTIFACT).expect("approved artifact must load");
    let left_inventory = parse_inventory(LEFT_INVENTORY_339);
    let dropped = "time";
    assert!(left_inventory.iter().any(|path| path == dropped));
    let right_inventory: Vec<String> = parse_inventory(RIGHT_INVENTORY_411)
        .into_iter()
        .filter(|path| path != dropped)
        .collect();

    let violations = map
        .check_completeness(&left_inventory, &right_inventory)
        .expect_err("dropping an identity-claimed path's counterpart must fail the proof");
    assert!(
        violations.contains(&CompletenessViolation::DefaultAssumesMissingCounterpart {
            side: InventorySide::Left,
            path: dropped.to_string(),
        }),
        "{violations:?}"
    );
}

#[test]
fn completeness_violations_do_not_influence_resolution() {
    let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <default rel="identical"/>
            </ids-map>
        "#;
    let map = ConversionMap::load(xml).expect("map loads");
    let left_inventory = vec!["a".to_string()];
    let right_inventory: Vec<String> = vec![]; // deliberately incomplete

    let before = map.resolve("a", Direction::Forward);
    assert!(
        map.check_completeness(&left_inventory, &right_inventory)
            .is_err()
    );
    let after = map.resolve("a", Direction::Forward);

    assert_eq!(before, after);
}
