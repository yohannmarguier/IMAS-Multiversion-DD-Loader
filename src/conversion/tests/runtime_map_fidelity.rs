use super::*;
use crate::conversion::conversion_map::{Direction, Outcome, ValueTransformation};
use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug)]
struct FidelityReport {
    source_identity: &'static str,
    request: MapRequest,
    checks: Vec<FidelityCheck>,
    mismatches: Vec<FidelityCheck>,
}

#[derive(Debug, Clone)]
struct FidelityCheck {
    path: String,
    expected: String,
    observed: String,
}

impl FidelityReport {
    fn to_json(&self) -> String {
        let mut json = String::new();
        write!(
            json,
            "{{\n  \"schema\": \"imas-mvdd-runtime-map-fidelity-v1\",\n  \"source_identity\": \"{}\",\n  \"request\": {{\"ids\": \"{}\", \"hli_dd\": \"{}\", \"stored_dd\": \"{}\"}},\n  \"checks\": [",
            self.source_identity,
            self.request.ids,
            self.request.hli_dd,
            self.request.stored_dd
        )
        .unwrap();
        for (index, check) in self.checks.iter().enumerate() {
            if index != 0 {
                json.push(',');
            }
            write!(
                json,
                "\n    {{\"path\": \"{}\", \"expected\": \"{}\", \"observed\": \"{}\"}}",
                json_string(&check.path),
                json_string(&check.expected),
                json_string(&check.observed)
            )
            .unwrap();
        }
        json.push_str("\n  ],\n  \"mismatches\": [");
        for (index, mismatch) in self.mismatches.iter().enumerate() {
            if index != 0 {
                json.push(',');
            }
            write!(
                json,
                "\n    {{\"path\": \"{}\", \"expected\": \"{}\", \"observed\": \"{}\"}}",
                json_string(&mismatch.path),
                json_string(&mismatch.expected),
                json_string(&mismatch.observed)
            )
            .unwrap();
        }
        json.push_str("\n  ]\n}");
        json
    }
}

fn write_reports_if_requested(reports: &[&FidelityReport]) {
    let Some(path) = std::env::var_os("IMAS_MVDD_FIDELITY_REPORT") else {
        return;
    };
    let path = Path::new(&path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("fidelity report directory must be writable");
    }
    let contents = format!(
        "{{\n  \"schema\": \"imas-mvdd-runtime-map-fidelity-bundle-v1\",\n  \"reports\": [\n{}\n  ]\n}}\n",
        reports
            .iter()
            .map(|report| {
                report
                    .to_json()
                    .lines()
                    .map(|line| format!("    {line}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .collect::<Vec<_>>()
            .join(",\n")
    );
    std::fs::write(path, contents).expect("fidelity report must be writable");
}

fn json_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

#[derive(Clone)]
struct OracleSource {
    facts: IdsGraphFacts,
}

impl GraphFactsSource for OracleSource {
    fn load_ids_facts(
        &self,
        _ids: &str,
        _attempt: &AcquisitionAttempt,
    ) -> Result<IdsGraphFacts, GraphSourceError> {
        Ok(self.facts.clone())
    }
}

#[derive(Debug)]
struct ExpectedPath {
    path: &'static str,
    observed: &'static str,
}

fn release(value: &str) -> ArtifactDdVersion {
    ArtifactDdVersion::new(value).expect("oracle releases are valid")
}

fn endpoint(release_text: &str, kind: GraphNodeKind, data_type: &str) -> EndpointMetadata {
    EndpointMetadata {
        release: release(release_text),
        kind,
        data_type: data_type.to_string(),
        ndim: if data_type.ends_with("_0D") { 0 } else { 1 },
        unit: None,
        timebase_path: None,
        coordinate_paths: Vec::new(),
        cocos_label_transformation: None,
        cocos_transformation_expression: None,
        cocos_label_source: None,
    }
}

fn node(path: &str, endpoints: Vec<EndpointMetadata>) -> GraphNode {
    GraphNode {
        ids: "oracle_ids".to_string(),
        path: path.to_string(),
        introduced: vec![release("3.39.0")],
        removed: Vec::new(),
        rename_declarations: Vec::new(),
        coordinate_relationships: Vec::new(),
        endpoints,
        source_metadata: None,
    }
}

fn controlled_scope() -> IdsGraphFacts {
    let mut old_name = node(
        "old_name",
        vec![endpoint("3.39.0", GraphNodeKind::Leaf, "INT_0D")],
    );
    old_name.removed = vec![release("4.0.0")];

    let mut new_name = node(
        "new_name",
        vec![
            endpoint("4.0.0", GraphNodeKind::Leaf, "INT_0D"),
            endpoint("4.1.1", GraphNodeKind::Leaf, "INT_0D"),
        ],
    );
    new_name.introduced = vec![release("4.0.0")];
    new_name.rename_declarations = vec![GraphRename {
        release: release("4.0.0"),
        previous_name: "old_name".to_string(),
    }];

    let mut old_only = node(
        "old_only",
        vec![endpoint("3.39.0", GraphNodeKind::Leaf, "INT_0D")],
    );
    old_only.removed = vec![release("4.0.0")];

    let mut new_only = node(
        "new_only",
        vec![
            endpoint("4.0.0", GraphNodeKind::Leaf, "INT_0D"),
            endpoint("4.1.1", GraphNodeKind::Leaf, "INT_0D"),
        ],
    );
    new_only.introduced = vec![release("4.0.0")];

    let mut unresolved = node(
        "unresolved",
        vec![
            endpoint("3.39.0", GraphNodeKind::Leaf, "INT_1D"),
            endpoint("4.1.1", GraphNodeKind::Structure, "STRUCT_ARRAY"),
        ],
    );
    unresolved.endpoints[1].ndim = 1;

    let mut psi = node(
        "psi",
        vec![
            endpoint("3.39.0", GraphNodeKind::Leaf, "FLT_0D"),
            endpoint("4.1.1", GraphNodeKind::Leaf, "FLT_0D"),
        ],
    );
    for endpoint in &mut psi.endpoints {
        endpoint.cocos_label_transformation = Some("psi_like".to_string());
        endpoint.cocos_label_source = Some(CocosLabelSource::InferredSignFlip);
    }

    IdsGraphFacts {
        complete: true,
        versions: vec![
            GraphVersion {
                release: release("3.39.0"),
                cocos: Some(CocosConvention::new("11").unwrap()),
            },
            GraphVersion {
                release: release("4.0.0"),
                cocos: Some(CocosConvention::new("17").unwrap()),
            },
            GraphVersion {
                release: release("4.1.1"),
                cocos: Some(CocosConvention::new("17").unwrap()),
            },
        ],
        nodes: vec![
            node(
                "identity",
                vec![
                    endpoint("3.39.0", GraphNodeKind::Leaf, "INT_0D"),
                    endpoint("4.1.1", GraphNodeKind::Leaf, "INT_0D"),
                ],
            ),
            old_name,
            new_name,
            old_only,
            new_only,
            unresolved,
            psi,
        ],
        events: vec![
            GraphEvent {
                id: "unresolved:data_type:4.1.1".to_string(),
                path: "unresolved".to_string(),
                release: release("4.1.1"),
                field: "data_type".to_string(),
                kind: "structure_changed".to_string(),
                old_value: Some("INT_1D".to_string()),
                new_value: Some("STRUCT_ARRAY".to_string()),
                unit_change: None,
                semantic_type: None,
                coordinate_evidence: None,
            },
            GraphEvent {
                id: "psi:cocos_label_transformation:4.0.0".to_string(),
                path: "psi".to_string(),
                release: release("4.0.0"),
                field: "cocos_label_transformation".to_string(),
                kind: "metadata_changed".to_string(),
                old_value: Some("psi".to_string()),
                new_value: Some(String::new()),
                unit_change: None,
                semantic_type: None,
                coordinate_evidence: None,
            },
        ],
        successors: vec![GraphSuccessor {
            from_path: "old_name".to_string(),
            to_path: "new_name".to_string(),
        }],
    }
}

fn observed(map: &ConversionMap, path: &str) -> String {
    let Some(explanation) = map.resolve(path, Direction::Forward) else {
        return "unclaimed".to_string();
    };
    match explanation.outcome {
        Outcome::Path {
            resolved_path,
            value_transformation,
            candidates,
        } => format!(
            "path:{resolved_path};rel:{:?};fidelity:{:?};transform:{};candidates:{:?}",
            explanation.rel,
            explanation.fidelity,
            match value_transformation {
                ValueTransformation::None => "none".to_string(),
                ValueTransformation::SignFlip {
                    from_cocos,
                    to_cocos,
                    direction,
                } => format!("sign:{from_cocos}->{to_cocos}:{direction:?}"),
            },
            candidates
                .iter()
                .map(|candidate| (&candidate.path, candidate.precedence))
                .collect::<Vec<_>>()
        ),
        Outcome::NoSource => format!(
            "no-source;rel:{:?};fidelity:{:?}",
            explanation.rel, explanation.fidelity
        ),
        Outcome::Refusal(reason) => format!(
            "refusal:{reason:?};rel:{:?};fidelity:{:?}",
            explanation.rel, explanation.fidelity
        ),
    }
}

fn compare(
    source_identity: &'static str,
    request: MapRequest,
    expectations: &[ExpectedPath],
) -> FidelityReport {
    compare_facts(source_identity, request, controlled_scope(), expectations)
}

fn compare_facts(
    source_identity: &'static str,
    request: MapRequest,
    facts: IdsGraphFacts,
    expectations: &[ExpectedPath],
) -> FidelityReport {
    let map = RuntimeMapCoordinator::new(OracleSource { facts })
        .acquire(&request)
        .expect("the controlled complete scope must acquire");
    compare_map(source_identity, request, &map, expectations)
}

fn compare_map(
    source_identity: &'static str,
    request: MapRequest,
    map: &ConversionMap,
    expectations: &[ExpectedPath],
) -> FidelityReport {
    let checks = expectations
        .iter()
        .map(|expectation| {
            let actual = observed(map, expectation.path);
            FidelityCheck {
                path: expectation.path.to_string(),
                expected: expectation.observed.to_string(),
                observed: actual,
            }
        })
        .collect::<Vec<_>>();
    let mismatches = checks
        .iter()
        .filter(|check| check.expected != check.observed)
        .cloned()
        .collect();
    FidelityReport {
        source_identity,
        request,
        checks,
        mismatches,
    }
}

fn controlled_fidelity_report() -> FidelityReport {
    let request = MapRequest {
        ids: "oracle_ids".to_string(),
        stored_dd: release("3.39.0"),
        hli_dd: release("4.1.1"),
    };
    compare(
        "controlled:runtime-map-fidelity-v1",
        request,
        &[
            ExpectedPath {
                path: "identity",
                observed: "path:identity;rel:Some(Identical);fidelity:Exact;transform:none;candidates:[]",
            },
            ExpectedPath {
                path: "new_name",
                observed: "path:old_name;rel:Some(Renamed);fidelity:Exact;transform:none;candidates:[]",
            },
            ExpectedPath {
                path: "new_only",
                observed: "refusal:Unmappable;rel:Some(LeftOnly);fidelity:Unmappable",
            },
            ExpectedPath {
                path: "unresolved",
                observed: "refusal:UnservableRetype;rel:Some(Retyped);fidelity:Unmappable",
            },
            ExpectedPath {
                path: "psi",
                observed: "path:psi;rel:Some(Identical);fidelity:Exact;transform:sign:11->17:ToHli;candidates:[]",
            },
        ],
    )
}

fn reverse_fidelity_report() -> FidelityReport {
    compare(
        "controlled:runtime-map-fidelity-v1",
        MapRequest {
            ids: "oracle_ids".to_string(),
            stored_dd: release("4.1.1"),
            hli_dd: release("3.39.0"),
        },
        &[
            ExpectedPath {
                path: "identity",
                observed: "path:identity;rel:Some(Identical);fidelity:Exact;transform:none;candidates:[]",
            },
            ExpectedPath {
                path: "old_name",
                observed: "path:new_name;rel:Some(Renamed);fidelity:Exact;transform:none;candidates:[]",
            },
            ExpectedPath {
                path: "old_only",
                observed: "refusal:Unmappable;rel:Some(LeftOnly);fidelity:Unmappable",
            },
            ExpectedPath {
                path: "unresolved",
                observed: "refusal:UnservableRetype;rel:Some(Retyped);fidelity:Unmappable",
            },
            ExpectedPath {
                path: "psi",
                observed: "path:psi;rel:Some(Identical);fidelity:Exact;transform:sign:17->11:ToHli;candidates:[]",
            },
        ],
    )
}

fn candidate_fidelity_report() -> FidelityReport {
    compare_facts(
        "controlled:coexistence-v1",
        MapRequest {
            ids: "equilibrium".to_string(),
            stored_dd: release("3.42.0"),
            hli_dd: release("4.1.1"),
        },
        super::tests::coexistence_facts(),
        &[ExpectedPath {
            path: "time_slice/constraints/j_phi",
            observed: "path:time_slice/constraints/j_phi;rel:Some(Split);fidelity:Exact;transform:none;candidates:[(\"time_slice/constraints/j_phi\", 1), (\"time_slice/constraints/j_tor\", 2)]",
        }],
    )
}

#[test]
fn controlled_fidelity_oracle_reports_the_exact_ordered_request() {
    let report = controlled_fidelity_report();
    let reverse = reverse_fidelity_report();
    let candidate = candidate_fidelity_report();

    assert_eq!(report.source_identity, "controlled:runtime-map-fidelity-v1");
    assert_eq!(report.request.ids, "oracle_ids");
    assert_eq!(report.request.hli_dd.to_string(), "4.1.1");
    assert_eq!(report.request.stored_dd.to_string(), "3.39.0");
    write_reports_if_requested(&[&report, &reverse, &candidate]);
    assert!(report.mismatches.is_empty(), "{report:#?}");
    assert!(reverse.mismatches.is_empty(), "{reverse:#?}");
    assert!(candidate.mismatches.is_empty(), "{candidate:#?}");
}

#[test]
fn controlled_fidelity_oracle_checks_the_reverse_request_independently() {
    let report = reverse_fidelity_report();

    assert_eq!(report.request.hli_dd.to_string(), "3.39.0");
    assert_eq!(report.request.stored_dd.to_string(), "4.1.1");
    assert!(report.mismatches.is_empty(), "{report:#?}");
}

#[test]
fn mismatch_records_name_the_path_expected_and_observed_semantics() {
    let report = compare(
        "controlled:runtime-map-fidelity-v1",
        MapRequest {
            ids: "oracle_ids".to_string(),
            stored_dd: release("3.39.0"),
            hli_dd: release("4.1.1"),
        },
        &[ExpectedPath {
            path: "new_name",
            observed: "path:wrong-target",
        }],
    );

    assert_eq!(report.mismatches.len(), 1);
    assert_eq!(report.mismatches[0].path, "new_name");
    assert_eq!(report.mismatches[0].expected, "path:wrong-target");
    assert!(report.mismatches[0].observed.contains("path:old_name"));
}

#[test]
fn mismatch_report_is_machine_readable_and_names_the_ordered_request() {
    let report = compare(
        "sha256:controlled-snapshot",
        MapRequest {
            ids: "oracle_ids".to_string(),
            stored_dd: release("3.39.0"),
            hli_dd: release("4.1.1"),
        },
        &[ExpectedPath {
            path: "new_name",
            observed: "path:altered-candidate",
        }],
    );
    let json = report.to_json();

    assert!(json.starts_with('{') && json.ends_with('}'));
    assert!(json.contains("\"source_identity\": \"sha256:controlled-snapshot\""));
    assert!(json.contains("\"hli_dd\": \"4.1.1\", \"stored_dd\": \"3.39.0\""));
    assert!(json.contains("\"path\": \"new_name\""));
    assert!(json.contains("\"expected\": \"path:altered-candidate\""));
    assert!(json.contains("path:old_name"));
}

#[derive(Clone)]
struct CountingSource {
    loads: Arc<AtomicUsize>,
}

impl GraphFactsSource for CountingSource {
    fn load_ids_facts(
        &self,
        ids: &str,
        _attempt: &AcquisitionAttempt,
    ) -> Result<IdsGraphFacts, GraphSourceError> {
        self.loads.fetch_add(1, Ordering::SeqCst);
        let mut facts = controlled_scope();
        for node in &mut facts.nodes {
            node.ids = ids.to_string();
        }
        Ok(facts)
    }
}

#[test]
fn fidelity_oracle_cache_keeps_ids_versions_and_direction_in_the_key() {
    let loads = Arc::new(AtomicUsize::new(0));
    let coordinator = RuntimeMapCoordinator::new(CountingSource {
        loads: Arc::clone(&loads),
    });
    let forward = MapRequest {
        ids: "oracle_ids".to_string(),
        stored_dd: release("3.39.0"),
        hli_dd: release("4.1.1"),
    };
    let reverse = MapRequest {
        stored_dd: forward.hli_dd.clone(),
        hli_dd: forward.stored_dd.clone(),
        ..forward.clone()
    };
    let other_ids = MapRequest {
        ids: "other_ids".to_string(),
        ..forward.clone()
    };
    let other_version = MapRequest {
        stored_dd: release("4.0.0"),
        ..forward.clone()
    };

    let first = coordinator.acquire(&forward).unwrap();
    drop(first);
    let warm = coordinator.acquire(&forward).unwrap();
    let reverse_map = coordinator.acquire(&reverse).unwrap();
    let other_map = coordinator.acquire(&other_ids).unwrap();
    let other_version_map = coordinator.acquire(&other_version).unwrap();

    assert_eq!(loads.load(Ordering::SeqCst), 4);
    assert!(Arc::ptr_eq(&warm, &coordinator.acquire(&forward).unwrap()));
    assert!(!Arc::ptr_eq(&warm, &reverse_map));
    assert!(!Arc::ptr_eq(&warm, &other_map));
    assert!(!Arc::ptr_eq(&warm, &other_version_map));
}

pub(crate) fn acquired_oracle_map() -> Arc<ConversionMap> {
    let request = MapRequest {
        ids: "oracle_ids".to_string(),
        stored_dd: release("3.39.0"),
        hli_dd: release("4.1.1"),
    };
    RuntimeMapCoordinator::new(OracleSource {
        facts: controlled_scope(),
    })
    .acquire(&request)
    .expect("the controlled runtime-map oracle must acquire")
}

#[test]
fn altered_cache_key_selection_is_a_named_negative_control() {
    let coordinator = RuntimeMapCoordinator::new(OracleSource {
        facts: controlled_scope(),
    });
    let forward = MapRequest {
        ids: "oracle_ids".to_string(),
        stored_dd: release("3.39.0"),
        hli_dd: release("4.1.1"),
    };
    let reverse = MapRequest {
        stored_dd: forward.hli_dd.clone(),
        hli_dd: forward.stored_dd.clone(),
        ..forward.clone()
    };
    let wrongly_selected = coordinator.acquire(&reverse).unwrap();
    let report = compare_map(
        "controlled:wrong-cache-key",
        forward,
        &wrongly_selected,
        &[ExpectedPath {
            path: "new_name",
            observed: "path:old_name;rel:Some(Renamed);fidelity:Exact;transform:none;candidates:[]",
        }],
    );

    assert_eq!(report.mismatches.len(), 1);
    assert_eq!(report.mismatches[0].path, "new_name");
    assert_eq!(report.mismatches[0].observed, "unclaimed");
}

#[test]
fn fidelity_oracle_records_candidate_target_order_and_precedence() {
    let report = candidate_fidelity_report();

    assert!(report.mismatches.is_empty(), "{report:#?}");
}

#[test]
fn altered_candidate_order_is_a_named_negative_control() {
    let report = compare_facts(
        "controlled:coexistence-v1",
        MapRequest {
            ids: "equilibrium".to_string(),
            stored_dd: release("3.42.0"),
            hli_dd: release("4.1.1"),
        },
        super::tests::coexistence_facts(),
        &[ExpectedPath {
            path: "time_slice/constraints/j_phi",
            observed: "path:time_slice/constraints/j_phi;rel:Some(Split);fidelity:Exact;transform:none;candidates:[(\"time_slice/constraints/j_tor\", 1), (\"time_slice/constraints/j_phi\", 2)]",
        }],
    );

    assert_eq!(report.mismatches.len(), 1);
    assert_eq!(report.mismatches[0].path, "time_slice/constraints/j_phi");
    assert!(report.mismatches[0].observed.contains("j_phi\", 1"));
}

#[test]
fn altered_sign_factor_is_a_named_negative_control() {
    let mut facts = controlled_scope();
    facts.versions[0].cocos = Some(CocosConvention::new("17").unwrap());
    let report = compare_facts(
        "controlled:runtime-map-fidelity-sign-control",
        MapRequest {
            ids: "oracle_ids".to_string(),
            stored_dd: release("3.39.0"),
            hli_dd: release("4.1.1"),
        },
        facts,
        &[ExpectedPath {
            path: "psi",
            observed: "path:psi;rel:Some(Identical);fidelity:Exact;transform:sign:11->17:ToHli;candidates:[]",
        }],
    );

    assert_eq!(report.mismatches.len(), 1);
    assert_eq!(report.mismatches[0].path, "psi");
    assert!(report.mismatches[0].observed.contains("transform:none"));
}

#[test]
fn altered_endpoint_inventory_fails_the_leaf_operation_assertion() {
    let request = MapRequest {
        ids: "oracle_ids".to_string(),
        stored_dd: release("3.39.0"),
        hli_dd: release("4.1.1"),
    };
    let baseline = RuntimeMapAcquirer::new(OracleSource {
        facts: controlled_scope(),
    })
    .acquire(&request)
    .unwrap();
    assert!(baseline.delete_target_is_leaf(Direction::Forward, "identity"));

    let mut altered = controlled_scope();
    let endpoint = &mut altered
        .nodes
        .iter_mut()
        .find(|node| node.path == "identity")
        .unwrap()
        .endpoints[1];
    endpoint.kind = GraphNodeKind::Structure;
    endpoint.data_type = "STRUCTURE".to_string();
    endpoint.ndim = 0;
    altered.events.push(GraphEvent {
        id: "identity:data_type:4.1.1".to_string(),
        path: "identity".to_string(),
        release: release("4.1.1"),
        field: "data_type".to_string(),
        kind: "structure_changed".to_string(),
        old_value: Some("INT_0D".to_string()),
        new_value: Some("STRUCTURE".to_string()),
        unit_change: None,
        semantic_type: None,
        coordinate_evidence: None,
    });
    let altered = RuntimeMapAcquirer::new(OracleSource { facts: altered })
        .acquire(&request)
        .unwrap();
    assert!(
        !altered.delete_target_is_leaf(Direction::Forward, "identity"),
        "altered endpoint inventory must change the observable delete contract"
    );
}
