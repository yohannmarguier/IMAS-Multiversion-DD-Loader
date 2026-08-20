use super::*;
use std::cell::Cell;
use std::thread;

const MINIMAL_ARTIFACT: &str = r#"
        <ids-map ids="equilibrium" format-version="1">
          <side id="left" dd="3.39.0" cocos="11"/>
          <side id="right" dd="4.1.1" cocos="17"/>
        </ids-map>
    "#;

fn dummy_map() -> ConversionMap {
    ConversionMap::load(MINIMAL_ARTIFACT).expect("minimal artifact must load")
}

fn version(input: &str) -> DdVersion {
    input.parse().expect("test DD version must be valid")
}

fn dummy_key() -> MapCacheKey {
    MapCacheKey::new(
        "equilibrium".to_string(),
        version("3.39.0"),
        version("4.1.1"),
    )
}

// dummy_key() pairs stored=3.39.0 with hli=4.1.1, which known_artifacts
// resolves as Direction::Reverse (the HLI's own, right-side spelling
// resolves to the stored, left-side spelling in reverse).
const DUMMY_DIRECTION: Direction = Direction::Reverse;

fn record_dummy_root(
    registry: &ContextRegistry,
    ctx_id: ContextId,
    resolved_path: String,
    pulse_ctx_id: ContextId,
) -> bool {
    registry.record_root(
        ctx_id,
        resolved_path,
        pulse_ctx_id,
        dummy_key(),
        DUMMY_DIRECTION,
        dummy_map,
    )
}

#[test]
fn an_unrecorded_context_has_no_conversion_record() {
    let registry = ContextRegistry::new();
    assert!(registry.lookup(1).is_none());
}

#[test]
fn a_root_record_retains_its_path_pulse_id_map_and_root_identity() {
    let registry = ContextRegistry::new();
    let key = dummy_key();
    assert!(registry.record_root(
        5,
        "time_slice/boundary/psi".to_string(),
        1,
        key.clone(),
        DUMMY_DIRECTION,
        dummy_map,
    ));

    let snapshot = registry.lookup(5).expect("just-recorded root must be live");
    assert_eq!(snapshot.resolved_path, "time_slice/boundary/psi");
    assert_eq!(snapshot.pulse_ctx_id, 1);
    assert_eq!(snapshot.root_id, 5, "a root's root identity is itself");
    assert_eq!(snapshot.stored_version.to_string(), "3.39.0");
    assert_eq!(snapshot.hli_version.to_string(), "4.1.1");
    assert!(
        Arc::ptr_eq(
            &snapshot.map,
            &registry.get_or_create_map(key, || panic!("map must be cached"))
        ),
        "lookup must hand back a shared reference to the same map, not a copy"
    );
}

#[test]
fn lookup_releases_the_lock_before_returning() {
    let registry = ContextRegistry::new();
    assert!(record_dummy_root(&registry, 5, "p".to_string(), 1));

    let _snapshot = registry.lookup(5).unwrap();
    // If `lookup` still held the lock at this point, this call would
    // deadlock rather than return.
    registry.remove(5);
    assert!(registry.lookup(5).is_none());
}

#[test]
fn removing_a_context_removes_only_that_exact_record() {
    let registry = ContextRegistry::new();
    assert!(record_dummy_root(&registry, 5, "a".to_string(), 1));
    assert!(record_dummy_root(&registry, 6, "b".to_string(), 1));

    registry.remove(5);

    assert!(registry.lookup(5).is_none());
    let survivor = registry.lookup(6).expect("removing 5 must not affect 6");
    assert_eq!(survivor.resolved_path, "b");
}

#[test]
fn removing_a_parent_record_does_not_invalidate_a_still_live_child() {
    let registry = ContextRegistry::new();
    assert!(record_dummy_root(&registry, 5, "root/path".to_string(), 1));
    assert!(registry.record_child(6, 5, "root/path/aos(1)".to_string()));

    registry.remove(5);

    let child = registry
        .lookup(6)
        .expect("removing the parent must not invalidate a still-live child");
    assert_eq!(child.resolved_path, "root/path/aos(1)");
    assert_eq!(child.root_id, 5, "child retains its resolved root identity");
    assert_eq!(child.pulse_ctx_id, 1, "child inherits the pulse context id");
}

#[test]
fn a_non_exact_read_from_a_child_is_retained_by_its_root_context() {
    let registry = ContextRegistry::new();
    assert!(record_dummy_root(&registry, 5, "root/path".to_string(), 1));
    assert!(registry.record_child(6, 5, "root/path/aos(1)".to_string()));

    let child = registry.lookup(6).expect("the child must be live");
    registry.record_read_loss_at_root(child.root_id, "field".to_string(), Fidelity::Lossy);

    let state = registry.state.lock().unwrap();
    assert_eq!(
        state.loss_logs.get(&5),
        Some(&vec![ReadLoss {
            hli_path: "field".to_string(),
            fidelity: Fidelity::Lossy,
        }])
    );
    assert!(!state.loss_logs.contains_key(&6));
}

#[test]
fn a_read_uses_its_captured_root_after_its_child_id_is_reused() {
    let registry = ContextRegistry::new();
    assert!(record_dummy_root(&registry, 5, "old/root".to_string(), 1));
    assert!(registry.record_child(6, 5, "old/root/aos(1)".to_string()));
    let read_root = registry.lookup(6).expect("the child must be live").root_id;

    // Model a child ending and its numeric ID being reused while
    // IMAS-Core is answering the read. A loss produced by the earlier
    // read must still belong to the root it captured, never this new root.
    registry.remove(6);
    assert!(record_dummy_root(
        &registry,
        6,
        "replacement/root".to_string(),
        2
    ));

    registry.record_read_loss_at_root(read_root, "old/root/field".to_string(), Fidelity::Lossy);

    assert_eq!(registry.loss_count(5), 1);
    assert_eq!(registry.loss_count(6), 0);
}

#[test]
fn a_child_record_retains_its_own_path_and_parent_id_and_shares_the_parents_map() {
    let registry = ContextRegistry::new();
    let key = dummy_key();
    assert!(registry.record_root(
        5,
        "root/path".to_string(),
        1,
        key.clone(),
        DUMMY_DIRECTION,
        dummy_map
    ));

    assert!(registry.record_child(6, 5, "root/path/aos(1)".to_string()));

    let child = registry
        .lookup(6)
        .expect("just-recorded child must be live");
    assert_eq!(child.resolved_path, "root/path/aos(1)");
    assert_eq!(child.pulse_ctx_id, 1, "child inherits the pulse context id");
    assert_eq!(child.root_id, 5);
    assert_eq!(child.parent_id, Some(5));
    assert_eq!(child.stored_version.to_string(), "3.39.0");
    assert_eq!(child.hli_version.to_string(), "4.1.1");
    assert_eq!(
        child.direction_to_stored, DUMMY_DIRECTION,
        "child inherits the parent's direction"
    );
    assert!(
        Arc::ptr_eq(
            &child.map,
            &registry.get_or_create_map(key, || panic!("map must be cached"))
        ),
        "child must share the same map reference as its parent, not a copy"
    );
}

#[test]
fn a_root_record_has_no_parent_id() {
    let registry = ContextRegistry::new();
    assert!(record_dummy_root(&registry, 5, "root/path".to_string(), 1));

    let root = registry.lookup(5).unwrap();
    assert_eq!(root.parent_id, None);
}

#[test]
fn a_grandchild_inherits_the_root_identity_through_its_immediate_parent() {
    let registry = ContextRegistry::new();
    assert!(record_dummy_root(&registry, 5, "root/path".to_string(), 1));
    assert!(registry.record_child(6, 5, "root/path/aos(1)".to_string()));

    assert!(registry.record_child(7, 6, "root/path/aos(1)/nested(2)".to_string()));

    let grandchild = registry.lookup(7).unwrap();
    assert_eq!(
        grandchild.root_id, 5,
        "root identity resolves through the chain of parents, not just the immediate one"
    );
    assert_eq!(
        grandchild.parent_id,
        Some(6),
        "parent id names the direct parent only"
    );
    assert_eq!(grandchild.pulse_ctx_id, 1);
}

#[test]
fn recording_a_child_under_an_id_with_no_live_conversion_record_fails_and_clears_any_stale_entry() {
    let registry = ContextRegistry::new();
    registry.record_dataentry(1);
    assert!(record_dummy_root(&registry, 9, "stale".to_string(), 1));

    // A data-entry context is not a conversion record: no root to inherit from.
    assert!(!registry.record_child(9, 1, "irrelevant".to_string()));
    assert!(
        registry.lookup(9).is_none(),
        "a failed child recording must clear whatever used to live at ctx_id"
    );

    // An unrecorded/recycled parent id behaves the same way.
    assert!(!registry.record_child(20, 999, "irrelevant".to_string()));
    assert!(registry.lookup(20).is_none());
}

#[test]
fn removing_a_child_affects_only_that_context_id() {
    let registry = ContextRegistry::new();
    assert!(record_dummy_root(&registry, 5, "root/path".to_string(), 1));
    assert!(registry.record_child(6, 5, "child/a".to_string()));
    assert!(registry.record_child(7, 5, "child/b".to_string()));

    registry.remove(6);

    assert!(registry.lookup(6).is_none());
    assert_eq!(registry.lookup(7).unwrap().resolved_path, "child/b");
    assert_eq!(registry.lookup(5).unwrap().resolved_path, "root/path");
}

#[test]
fn a_recycled_child_id_never_exposes_the_record_it_used_to_name() {
    let registry = ContextRegistry::new();
    assert!(record_dummy_root(&registry, 5, "root/path".to_string(), 1));
    assert!(registry.record_child(6, 5, "old/child".to_string()));
    registry.remove(6);

    assert!(registry.record_child(6, 5, "new/child".to_string()));

    let snapshot = registry.lookup(6).unwrap();
    assert_eq!(snapshot.resolved_path, "new/child");
}

#[test]
fn a_recycled_parent_id_does_not_retroactively_change_an_already_recorded_child() {
    let registry = ContextRegistry::new();
    assert!(record_dummy_root(&registry, 5, "old/root".to_string(), 1));
    assert!(registry.record_child(6, 5, "old/child".to_string()));
    registry.remove(5);

    // Id 5 is recycled for an unrelated new root.
    assert!(record_dummy_root(&registry, 5, "new/root".to_string(), 2));

    let child = registry
        .lookup(6)
        .expect("the child recorded against the old parent stays live");
    assert_eq!(
        child.resolved_path, "old/child",
        "the child's own snapshot must not shift just because id 5 was recycled"
    );
    assert_eq!(
        child.root_id, 5,
        "root_id is the numeric id resolved at record time"
    );
    assert_eq!(
        child.pulse_ctx_id, 1,
        "the child keeps the pulse it inherited originally"
    );
}

#[test]
fn child_lookup_releases_the_lock_before_returning() {
    let registry = ContextRegistry::new();
    assert!(record_dummy_root(&registry, 5, "root/path".to_string(), 1));
    assert!(registry.record_child(6, 5, "child".to_string()));

    let _snapshot = registry.lookup(6).unwrap();
    // If `lookup` still held the lock at this point, this call would
    // deadlock rather than return.
    registry.remove(6);
    assert!(registry.lookup(6).is_none());
}

#[test]
fn a_recycled_context_id_identifies_only_the_newest_root_record() {
    let registry = ContextRegistry::new();
    assert!(record_dummy_root(&registry, 5, "old/path".to_string(), 1));
    registry.remove(5);

    assert!(record_dummy_root(&registry, 5, "new/path".to_string(), 2));

    let snapshot = registry
        .lookup(5)
        .expect("the recycled id must resolve to the newest record");
    assert_eq!(snapshot.resolved_path, "new/path");
    assert_eq!(snapshot.pulse_ctx_id, 2);
}

#[test]
fn recording_over_a_live_id_replaces_it_without_removal() {
    let registry = ContextRegistry::new();
    assert!(record_dummy_root(&registry, 5, "old/path".to_string(), 1));

    // No `remove` call in between: recording at a still-live ID must
    // still fully replace it, not merge with or expose the old record.
    assert!(record_dummy_root(&registry, 5, "new/path".to_string(), 2));

    let snapshot = registry.lookup(5).unwrap();
    assert_eq!(snapshot.resolved_path, "new/path");
}

#[test]
fn a_dataentry_context_carries_no_stored_version_and_no_map() {
    let registry = ContextRegistry::new();
    registry.record_dataentry(10);

    // Not a conversion record: no path, no map, no rule resolution
    // triggered by its presence alone.
    assert!(registry.lookup(10).is_none());
    // But it does supply its own ID as the pulse context ID that
    // operation records opened beneath it will carry.
    assert_eq!(registry.pulse_ctx_id(10), Some(10));
}

#[test]
fn a_dataentry_context_never_by_itself_is_conversion_eligible() {
    let registry = ContextRegistry::new();
    registry.record_dataentry(10);

    // An operation record opened under this pulse carries its ID
    // faithfully, but the pulse's mere presence never manufactured a
    // conversion record for its own ID.
    let pulse_id = registry.pulse_ctx_id(10).expect("live data-entry context");
    assert!(record_dummy_root(&registry, 11, "p".to_string(), pulse_id));

    assert!(registry.lookup(10).is_none());
    let root = registry.lookup(11).unwrap();
    assert_eq!(root.pulse_ctx_id, 10);
}

#[test]
fn pulse_ctx_id_is_none_for_a_conversion_record_or_unrecorded_id() {
    let registry = ContextRegistry::new();
    assert!(record_dummy_root(&registry, 5, "p".to_string(), 1));

    assert_eq!(registry.pulse_ctx_id(5), None);
    assert_eq!(registry.pulse_ctx_id(999), None);
}

#[test]
fn matching_versions_remove_stale_records_without_creating_a_map() {
    let registry = ContextRegistry::new();
    let loads = Cell::new(0);
    assert!(record_dummy_root(&registry, 5, "stale".to_string(), 1));
    let matching_key = MapCacheKey::new(
        "equilibrium".to_string(),
        version("4.1.1"),
        version("4.1.1"),
    );

    assert!(
        !registry.record_root(5, "p".to_string(), 1, matching_key, DUMMY_DIRECTION, || {
            loads.set(loads.get() + 1);
            dummy_map()
        },)
    );

    assert!(
        registry.lookup(5).is_none(),
        "matching versions need no record"
    );
    assert_eq!(loads.get(), 0, "matching versions must not load a map");
}

#[test]
fn a_shared_map_survives_as_long_as_one_record_still_references_it() {
    let registry = ContextRegistry::new();
    let loads = Cell::new(0);
    let key = dummy_key();

    assert!(
        registry.record_root(5, "a".to_string(), 1, key.clone(), DUMMY_DIRECTION, || {
            loads.set(loads.get() + 1);
            dummy_map()
        })
    );
    assert!(
        registry.record_root(6, "b".to_string(), 1, key.clone(), DUMMY_DIRECTION, || {
            loads.set(loads.get() + 1);
            dummy_map()
        })
    );

    assert_eq!(loads.get(), 1, "the second record must hit the cache");

    registry.remove(5);
    let survivor = registry.lookup(6).unwrap();
    let map_after_one_removed = registry.get_or_create_map(key, || {
        loads.set(loads.get() + 1);
        dummy_map()
    });
    assert_eq!(
        loads.get(),
        1,
        "the map must stay cached while record 6 still references it"
    );
    assert!(Arc::ptr_eq(&survivor.map, &map_after_one_removed));
}

#[test]
fn a_shared_map_is_released_once_no_record_references_it() {
    let registry = ContextRegistry::new();
    let loads = Cell::new(0);
    let key = dummy_key();

    assert!(
        registry.record_root(5, "a".to_string(), 1, key.clone(), DUMMY_DIRECTION, || {
            loads.set(loads.get() + 1);
            dummy_map()
        })
    );
    registry.remove(5);

    let _new_map = registry.get_or_create_map(key, || {
        loads.set(loads.get() + 1);
        dummy_map()
    });
    assert_eq!(
        loads.get(),
        2,
        "with no record left referencing it, the map must be released and recreated"
    );
}

#[test]
fn concurrent_operations_never_observe_a_torn_record() {
    let registry = Arc::new(ContextRegistry::new());
    let mut handles = Vec::new();

    for i in 0..8 {
        let registry = registry.clone();
        handles.push(thread::spawn(move || {
            let ctx_id = i as ContextId;
            for _ in 0..200 {
                registry.record_root(
                    ctx_id,
                    format!("path-{i}"),
                    ctx_id,
                    dummy_key(),
                    DUMMY_DIRECTION,
                    dummy_map,
                );
                if let Some(snapshot) = registry.lookup(ctx_id) {
                    // A torn record would show a path that does not
                    // match the pulse ID this thread always pairs it
                    // with.
                    assert_eq!(snapshot.resolved_path, format!("path-{i}"));
                    assert_eq!(snapshot.pulse_ctx_id, ctx_id);
                }
                registry.remove(ctx_id);
            }
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn concurrent_child_operations_never_observe_a_torn_record() {
    let registry = Arc::new(ContextRegistry::new());
    let mut handles = Vec::new();

    for i in 0..8 {
        let registry = registry.clone();
        handles.push(thread::spawn(move || {
            let root_id = i as ContextId;
            let child_id = (i + 100) as ContextId;
            for _ in 0..200 {
                registry.record_root(
                    root_id,
                    format!("root-{i}"),
                    root_id,
                    dummy_key(),
                    DUMMY_DIRECTION,
                    dummy_map,
                );
                registry.record_child(child_id, root_id, format!("child-{i}"));
                if let Some(snapshot) = registry.lookup(child_id) {
                    // A torn record would show a path or root identity
                    // that does not match the root this thread always
                    // pairs it with.
                    assert_eq!(snapshot.resolved_path, format!("child-{i}"));
                    assert_eq!(snapshot.root_id, root_id);
                    assert_eq!(snapshot.parent_id, Some(root_id));
                }
                registry.remove(child_id);
                registry.remove(root_id);
            }
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn an_occurrence_never_seen_has_no_known_stored_version() {
    let registry = ContextRegistry::new();
    registry.record_dataentry(10);

    assert_eq!(registry.known_stored_version(10, "equilibrium"), None);
}

#[test]
fn a_remembered_mismatch_is_returned_by_known_stored_version() {
    let registry = ContextRegistry::new();
    registry.record_dataentry(10);

    registry.remember_mismatched_occurrence(10, "equilibrium".to_string(), version("3.39.0"));

    assert_eq!(
        registry.known_stored_version(10, "equilibrium"),
        Some(version("3.39.0"))
    );
    // A distinct occurrence under the same pulse is unaffected.
    assert_eq!(registry.known_stored_version(10, "core_profiles"), None);
}

#[test]
fn forgetting_an_occurrence_clears_only_that_occurrence() {
    let registry = ContextRegistry::new();
    registry.record_dataentry(10);
    registry.remember_mismatched_occurrence(10, "equilibrium".to_string(), version("3.39.0"));
    registry.remember_mismatched_occurrence(10, "core_profiles".to_string(), version("3.39.0"));

    registry.forget_occurrence_version(10, "equilibrium");

    assert_eq!(registry.known_stored_version(10, "equilibrium"), None);
    assert_eq!(
        registry.known_stored_version(10, "core_profiles"),
        Some(version("3.39.0"))
    );
}

#[test]
fn forgetting_an_unremembered_occurrence_is_a_no_op() {
    let registry = ContextRegistry::new();
    registry.record_dataentry(10);

    registry.forget_occurrence_version(10, "equilibrium");

    assert_eq!(registry.known_stored_version(10, "equilibrium"), None);
}

#[test]
fn occurrence_version_methods_are_no_ops_for_a_non_dataentry_or_unrecorded_id() {
    let registry = ContextRegistry::new();
    assert!(record_dummy_root(&registry, 5, "p".to_string(), 1));

    // A conversion record is not a data-entry context: nothing to cache
    // onto, and looking it up must not panic or silently succeed.
    registry.remember_mismatched_occurrence(5, "equilibrium".to_string(), version("3.39.0"));
    assert_eq!(registry.known_stored_version(5, "equilibrium"), None);
    registry.forget_occurrence_version(5, "equilibrium");

    // An entirely unrecorded ID behaves the same way.
    registry.remember_mismatched_occurrence(999, "equilibrium".to_string(), version("3.39.0"));
    assert_eq!(registry.known_stored_version(999, "equilibrium"), None);
}

#[test]
fn recording_a_fresh_dataentry_at_a_recycled_id_resets_its_occurrence_cache() {
    let registry = ContextRegistry::new();
    registry.record_dataentry(10);
    registry.remember_mismatched_occurrence(10, "equilibrium".to_string(), version("3.39.0"));

    // A new pulse reusing the same context ID must not inherit the old
    // pulse's discoveries.
    registry.record_dataentry(10);

    assert_eq!(registry.known_stored_version(10, "equilibrium"), None);
}

#[test]
fn loss_count_is_zero_for_an_untracked_context() {
    let registry = ContextRegistry::new();
    registry.record_dataentry(10);

    // A data-entry context, an unrecorded id, and (by the same code
    // path) an operation whose versions matched all report zero rather
    // than a refusal: none of them ever produced a loss entry.
    assert_eq!(registry.loss_count(10), 0);
    assert_eq!(registry.loss_count(999), 0);
}

#[test]
fn loss_count_reports_the_retained_entries_on_a_root() {
    let registry = ContextRegistry::new();
    assert!(record_dummy_root(&registry, 5, "root/path".to_string(), 1));

    registry.record_read_loss_at_root(5, "field/a".to_string(), Fidelity::Lossy);
    registry.record_read_loss_at_root(5, "field/b".to_string(), Fidelity::Unmappable);

    assert_eq!(registry.loss_count(5), 2);
}

#[test]
fn loss_count_never_counts_an_exact_read() {
    let registry = ContextRegistry::new();
    assert!(record_dummy_root(&registry, 5, "root/path".to_string(), 1));

    registry.record_read_loss_at_root(5, "field/a".to_string(), Fidelity::Exact);

    assert_eq!(registry.loss_count(5), 0);
}

#[test]
fn loss_count_resolves_a_child_context_to_its_root() {
    let registry = ContextRegistry::new();
    assert!(record_dummy_root(&registry, 5, "root/path".to_string(), 1));
    assert!(registry.record_child(6, 5, "root/path/aos(1)".to_string()));

    registry.record_read_loss_at_root(5, "field".to_string(), Fidelity::Lossy);

    assert_eq!(
        registry.loss_count(6),
        1,
        "a query on the child must resolve to the same root log"
    );
    assert_eq!(registry.loss_count(5), 1);
}

#[test]
fn loss_at_returns_entries_in_the_order_they_were_recorded() {
    let registry = ContextRegistry::new();
    assert!(record_dummy_root(&registry, 5, "root/path".to_string(), 1));

    registry.record_read_loss_at_root(5, "field/a".to_string(), Fidelity::Lossy);
    registry.record_read_loss_at_root(5, "field/b".to_string(), Fidelity::Unmappable);

    assert_eq!(
        registry.with_loss_at(5, 0, |path, fidelity| (path.to_string(), fidelity)),
        Some(("field/a".to_string(), Fidelity::Lossy))
    );
    assert_eq!(
        registry.with_loss_at(5, 1, |path, fidelity| (path.to_string(), fidelity)),
        Some(("field/b".to_string(), Fidelity::Unmappable))
    );
}

#[test]
fn loss_at_returns_none_past_the_last_entry() {
    let registry = ContextRegistry::new();
    assert!(record_dummy_root(&registry, 5, "root/path".to_string(), 1));
    registry.record_read_loss_at_root(5, "field/a".to_string(), Fidelity::Lossy);

    assert_eq!(registry.with_loss_at(5, 1, |_, _| ()), None);
}

#[test]
fn loss_at_returns_none_for_any_index_on_an_untracked_context() {
    let registry = ContextRegistry::new();
    registry.record_dataentry(10);

    assert_eq!(registry.with_loss_at(10, 0, |_, _| ()), None);
    assert_eq!(registry.with_loss_at(999, 0, |_, _| ()), None);
}

#[test]
fn ending_the_root_context_destroys_its_loss_log() {
    let registry = ContextRegistry::new();
    assert!(record_dummy_root(&registry, 5, "root/path".to_string(), 1));
    registry.record_read_loss_at_root(5, "field/a".to_string(), Fidelity::Lossy);
    assert_eq!(registry.loss_count(5), 1);

    registry.remove(5);

    assert_eq!(registry.loss_count(5), 0);
    assert_eq!(registry.with_loss_at(5, 0, |_, _| ()), None);
}

#[test]
fn the_loss_log_dies_with_the_root_even_when_a_child_closes_non_lifo() {
    let registry = ContextRegistry::new();
    assert!(record_dummy_root(&registry, 5, "root/path".to_string(), 1));
    assert!(registry.record_child(6, 5, "root/path/aos(1)".to_string()));
    assert!(registry.record_child(7, 5, "root/path/aos(2)".to_string()));
    registry.record_read_loss_at_root(5, "field/a".to_string(), Fidelity::Lossy);
    registry.record_read_loss_at_root(5, "field/b".to_string(), Fidelity::Unmappable);
    assert_eq!(registry.loss_count(5), 2);

    // The root ends first — non-LIFO relative to the usual inner-to-outer
    // closing order — while both children are still live records.
    registry.remove(5);

    assert_eq!(registry.loss_count(5), 0);
    assert_eq!(
        registry.loss_count(6),
        0,
        "a child outliving its root must not resurrect the log"
    );
    assert_eq!(registry.loss_count(7), 0);
    assert!(
        registry.lookup(6).is_some(),
        "removing the root must not itself remove a still-live child record"
    );
    assert!(registry.lookup(7).is_some());
}
