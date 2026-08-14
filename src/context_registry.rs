//! Thread-safe context registry (issue #52, ADR 0003).
//!
//! One registry owns every live context whose stored DD version differs
//! from the HLI DD version, plus the pulse (data-entry) contexts operation
//! records are opened under. IMAS-Core hands out context IDs from one
//! shared live namespace, so a raw context ID identifies at most one live
//! record here — recording under a recycled ID replaces whatever used that
//! ID before, and looking it up never sees the old record.
//!
//! A record retains everything a later conversion needs — the resolved
//! absolute HLI-DD path, the pulse context ID, a shared conversion-map
//! reference, and its root identity — so a lookup is one operation rather
//! than a walk. `lookup` returns a cloned snapshot and a cheap `Arc` clone
//! of the map, then drops the lock before the caller does anything that
//! could call back into IMAS-Core or take a while (CONTEXT.md's "context
//! registry"; ADR 0003).
//!
//! A data-entry context (the pulse opened by `al_begin_dataentry_action`)
//! is recorded with no stored DD version and no conversion-map reference of
//! its own. It exists only so operation records opened under it can carry
//! its context ID as their pulse context ID; recording one never resolves
//! rules or transforms anything by itself. It also carries a small cache of
//! discovered occurrence versions (issue #53), keyed by `dataobjectname`: the
//! fact that a stored version, once found to mismatch the HLI DD version,
//! lets a later re-open of that same occurrence translate
//! `al_begin_global_action`'s `datapath` argument *before* IMAS-Core is
//! called, when the version-stamp read that would otherwise discover it
//! cannot happen until after the open (ADR 0002). This cache lives on the
//! data-entry entry rather than freestanding precisely so recording a new
//! data-entry context at a recycled ID resets it along with everything else
//! at that ID.
//!
//! A root's root identity is its own context ID. A child (arraystruct)
//! record resolves its root identity, pulse context ID, and shared
//! conversion map from a live parent snapshot instead of storing them
//! independently, and additionally carries its direct parent's context ID.
//! The parent snapshot only supplies that starting state — it does not make
//! the parent record own the child's lifecycle, and the registry exposes no
//! sibling enumeration or general ancestry-walking operation.
//!
//! The conversion-map cache is registry-owned and keyed by `(IDS name,
//! stored DD version, HLI DD version)`: it hands out `Arc` clones of a
//! shared `ConversionMap` and holds only a `Weak` reference itself, so a
//! map stays alive exactly as long as some record references it and is
//! dropped once none do — no explicit eviction needed.
//!
//! The data-entry and global-action seams register roots (issue #53), and
//! `al_begin_arraystruct_action` registers their live conversion-record
//! children (issue #54). `al_read_data` then looks up either record to
//! resolve its field in the stored DD's spelling.
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, Weak};

use crate::conversion_map::{ConversionMap, Direction, Fidelity};
use crate::dd_version::DdVersion;

/// An IMAS-Core context ID, as passed across the C ABI.
pub(crate) type ContextId = std::ffi::c_int;

/// The `(IDS name, stored DD version, HLI DD version)` key a shared
/// conversion map is cached under. Its validated version values make it
/// impossible to cache an invalid or noncanonical version spelling.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct MapCacheKey {
    ids: String,
    stored_version: DdVersion,
    hli_version: DdVersion,
}

impl MapCacheKey {
    pub(crate) fn new(ids: String, stored_version: DdVersion, hli_version: DdVersion) -> Self {
        Self {
            ids,
            stored_version,
            hli_version,
        }
    }

    fn needs_conversion(&self) -> bool {
        self.stored_version != self.hli_version
    }
}

/// A live conversion-eligible context: a root or a child.
#[derive(Debug, Clone)]
pub(crate) struct ConversionRecord {
    /// The path this context resolves to, in the HLI's own DD spelling,
    /// already made absolute.
    pub resolved_path: String,
    /// The data-entry context this record's pulse belongs to.
    pub pulse_ctx_id: ContextId,
    /// The conversion map this record resolves paths and values through,
    /// shared with every other record on the same version pair.
    pub map: Arc<ConversionMap>,
    /// The context ID of this record's root. Equal to the record's own
    /// context ID for a root record.
    pub root_id: ContextId,
    /// The direction (per the shared conversion map's own left/right sides)
    /// that resolves a path expressed in the HLI's own DD spelling to the
    /// stored DD spelling. Inherited unchanged from a root by every child.
    pub direction_to_stored: Direction,
    /// The stored DD version for this occurrence. It is retained with the
    /// record so a later rule refusal can identify both ends of the failed
    /// conversion without reopening or rediscovering the occurrence.
    pub stored_version: DdVersion,
    /// The DD version the calling HLI declared for this process. Like the
    /// stored version, this is inherited by child contexts for diagnostics.
    pub hli_version: DdVersion,
    /// The context ID of this record's direct parent, or `None` for a root
    /// record.
    parent_id: Option<ContextId>,
}

/// One entry in the registry's shared context-ID namespace.
#[derive(Debug, Clone)]
enum Entry {
    /// A pulse context: no stored DD version, no conversion map, not
    /// itself conversion-eligible. Carries the occurrence-version cache
    /// described in this module's doc comment, keyed by `dataobjectname`.
    DataEntry(HashMap<String, DdVersion>),
    /// A conversion-eligible context.
    Conversion(ConversionRecord),
}

/// A non-exact path requested through one root conversion context.
/// The log is root-owned so child contexts contribute to the same eventual
/// conversion report (ADR 0012); its public query ABI is deferred to #65.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ReadLoss {
    hli_path: String,
    fidelity: Fidelity,
}

#[derive(Default)]
struct State {
    entries: HashMap<ContextId, Entry>,
    /// Losses are separate from cloned conversion snapshots so a child never
    /// accidentally owns a copied log. A root context owns exactly one log.
    loss_logs: HashMap<ContextId, Vec<ReadLoss>>,
    maps: HashMap<MapCacheKey, Weak<ConversionMap>>,
}

/// The single shim-owned catalogue of live mismatched contexts (CONTEXT.md's
/// "context registry"). Every operation locks internally and returns owned
/// data — the lock itself and the entries map are never exposed.
#[derive(Default)]
pub(crate) struct ContextRegistry {
    state: Mutex<State>,
}

impl ContextRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Records `ctx_id` as a data-entry (pulse) context. Replaces whatever
    /// record — of any kind — previously lived at `ctx_id`, so a recycled ID
    /// starts with an empty occurrence-version cache rather than inheriting
    /// a previous pulse's discoveries.
    pub(crate) fn record_dataentry(&self, ctx_id: ContextId) {
        let mut state = self.state.lock().unwrap();
        state
            .entries
            .insert(ctx_id, Entry::DataEntry(HashMap::new()));
        state.loss_logs.remove(&ctx_id);
    }

    /// Records `ctx_id` as a root conversion record when the stored and HLI
    /// DD versions differ. Obtains the map through this registry's cache, so
    /// every record for one version pair shares a map. Returns `false` for a
    /// matching version pair after removing any record at `ctx_id`, so a
    /// recycled matching-version ID can never expose stale conversion state.
    ///
    /// For a mismatched pair, replaces whatever record — of any kind —
    /// previously lived at `ctx_id`, so a recycled ID can never expose the
    /// record it used to name.
    pub(crate) fn record_root(
        &self,
        ctx_id: ContextId,
        resolved_path: String,
        pulse_ctx_id: ContextId,
        key: MapCacheKey,
        direction_to_stored: Direction,
        create: impl FnOnce() -> ConversionMap,
    ) -> bool {
        if !key.needs_conversion() {
            self.remove(ctx_id);
            return false;
        }
        let stored_version = key.stored_version.clone();
        let hli_version = key.hli_version.clone();
        let record = ConversionRecord {
            resolved_path,
            pulse_ctx_id,
            map: self.get_or_create_map(key, create),
            root_id: ctx_id,
            direction_to_stored,
            stored_version,
            hli_version,
            parent_id: None,
        };
        let mut state = self.state.lock().unwrap();
        state.entries.insert(ctx_id, Entry::Conversion(record));
        state.loss_logs.insert(ctx_id, Vec::new());
        true
    }

    /// Records `ctx_id` as a child conversion record beneath the live
    /// conversion record at `parent_ctx_id`, inheriting its pulse context ID,
    /// shared conversion map, and root identity. `resolved_path` is this
    /// child's own resolved absolute HLI-DD path.
    ///
    /// Returns `false` and removes any record at `ctx_id` if `parent_ctx_id`
    /// names no live conversion record — a data-entry context, an unrecorded
    /// or recycled ID, or a root that turned out not to need conversion — so
    /// a recycled child ID never exposes stale state.
    ///
    /// Replaces whatever record — of any kind — previously lived at
    /// `ctx_id`, mirroring `record_root`.
    pub(crate) fn record_child(
        &self,
        ctx_id: ContextId,
        parent_ctx_id: ContextId,
        resolved_path: String,
    ) -> bool {
        let mut state = self.state.lock().unwrap();
        let parent = match state.entries.get(&parent_ctx_id) {
            Some(Entry::Conversion(record)) => record.clone(),
            Some(Entry::DataEntry(_)) | None => {
                state.entries.remove(&ctx_id);
                state.loss_logs.remove(&ctx_id);
                return false;
            }
        };
        state.loss_logs.remove(&ctx_id);
        state.entries.insert(
            ctx_id,
            Entry::Conversion(ConversionRecord {
                resolved_path,
                pulse_ctx_id: parent.pulse_ctx_id,
                map: parent.map,
                root_id: parent.root_id,
                direction_to_stored: parent.direction_to_stored,
                stored_version: parent.stored_version,
                hli_version: parent.hli_version,
                parent_id: Some(parent_ctx_id),
            }),
        );
        true
    }

    /// Returns a cloned snapshot of the conversion record at `ctx_id`, or
    /// `None` if `ctx_id` names no live context, a data-entry context, or a
    /// record that no longer exists there because its ID was recycled.
    ///
    /// The lock is released before this returns, so the caller is always
    /// free to call back into the registry, IMAS-Core, or transform data.
    pub(crate) fn lookup(&self, ctx_id: ContextId) -> Option<ConversionRecord> {
        match self.state.lock().unwrap().entries.get(&ctx_id) {
            Some(Entry::Conversion(record)) => Some(record.clone()),
            Some(Entry::DataEntry(_)) | None => None,
        }
    }

    /// Returns the pulse context ID `ctx_id` supplies to records opened
    /// beneath it — its own ID — if `ctx_id` names a live data-entry
    /// context; `None` otherwise, including for a conversion record or an
    /// unrecorded/recycled ID.
    pub(crate) fn pulse_ctx_id(&self, ctx_id: ContextId) -> Option<ContextId> {
        match self.state.lock().unwrap().entries.get(&ctx_id) {
            Some(Entry::DataEntry(_)) => Some(ctx_id),
            Some(Entry::Conversion(_)) | None => None,
        }
    }

    /// Returns the stored DD version already discovered for `dataobjectname`
    /// under the live data-entry context `pulse_ctx_id`, if a prior open of
    /// that same occurrence found one that mismatches the HLI DD version.
    /// `None` covers an occurrence never seen before, one already known to
    /// match (or be unstamped), and a `pulse_ctx_id` that names no live
    /// data-entry context — every one of these means "nothing to translate
    /// yet," which is exactly how `al_begin_global_action` treats it (issue
    /// #53, ADR 0002).
    pub(crate) fn known_stored_version(
        &self,
        pulse_ctx_id: ContextId,
        dataobjectname: &str,
    ) -> Option<DdVersion> {
        match self.state.lock().unwrap().entries.get(&pulse_ctx_id) {
            Some(Entry::DataEntry(known)) => known.get(dataobjectname).cloned(),
            _ => None,
        }
    }

    /// Records that `dataobjectname` under `pulse_ctx_id` is now known to be
    /// stored at `version`, which differs from the HLI DD version — the fact
    /// a later re-open of the same occurrence needs to translate
    /// `al_begin_global_action`'s `datapath` before calling IMAS-Core. A
    /// no-op if `pulse_ctx_id` no longer names a live data-entry context.
    pub(crate) fn remember_mismatched_occurrence(
        &self,
        pulse_ctx_id: ContextId,
        dataobjectname: String,
        version: DdVersion,
    ) {
        if let Some(Entry::DataEntry(known)) =
            self.state.lock().unwrap().entries.get_mut(&pulse_ctx_id)
        {
            known.insert(dataobjectname, version);
        }
    }

    /// Forgets any previously discovered mismatch for `dataobjectname` under
    /// `pulse_ctx_id` — called when a later open finds the occurrence now
    /// matches the HLI DD version, so a stale mismatch can never linger and
    /// wrongly translate a future `datapath`. A no-op if nothing was cached
    /// or `pulse_ctx_id` no longer names a live data-entry context.
    pub(crate) fn forget_occurrence_version(&self, pulse_ctx_id: ContextId, dataobjectname: &str) {
        if let Some(Entry::DataEntry(known)) =
            self.state.lock().unwrap().entries.get_mut(&pulse_ctx_id)
        {
            known.remove(dataobjectname);
        }
    }

    /// Appends a non-exact read outcome to its root context's loss log.
    /// Exact reads never enter the log; a missing, ended, or non-conversion
    /// context has no conversion loss to retain.
    pub(crate) fn record_read_loss(&self, ctx_id: ContextId, hli_path: String, fidelity: Fidelity) {
        if fidelity == Fidelity::Exact {
            return;
        }
        let mut state = self.state.lock().unwrap();
        let Some(Entry::Conversion(record)) = state.entries.get(&ctx_id) else {
            return;
        };
        let root_id = record.root_id;
        if let Some(losses) = state.loss_logs.get_mut(&root_id) {
            losses.push(ReadLoss { hli_path, fidelity });
        }
    }

    /// Returns the number of loss-log entries retained for `ctx_id`'s root
    /// context (ADR 0012), resolving a child to its root exactly as
    /// `record_read_loss` does. Reports `0` — never a refusal — for a
    /// data-entry context, an unrecorded or already-ended id, and an
    /// operation whose stored and HLI DD versions matched and was therefore
    /// never registered: every one of these has produced no loss entry, so
    /// zero is the truthful answer rather than an error.
    pub(crate) fn loss_count(&self, ctx_id: ContextId) -> usize {
        let state = self.state.lock().unwrap();
        let Some(Entry::Conversion(record)) = state.entries.get(&ctx_id) else {
            return 0;
        };
        state.loss_logs.get(&record.root_id).map_or(0, Vec::len)
    }

    /// Calls `read` with the `index`-th loss-log entry retained for `ctx_id`'s
    /// root context, in the order `record_read_loss` appended them, or
    /// returns `None` for an out-of-range index. Holding the registry lock
    /// only for the callback lets query exports copy directly from the
    /// retained string instead of cloning it onto the heap. This single
    /// bounds check also covers every untracked `ctx_id`, whose count is
    /// always zero.
    pub(crate) fn with_loss_at<T>(
        &self,
        ctx_id: ContextId,
        index: usize,
        read: impl FnOnce(&str, Fidelity) -> T,
    ) -> Option<T> {
        let state = self.state.lock().unwrap();
        let Some(Entry::Conversion(record)) = state.entries.get(&ctx_id) else {
            return None;
        };
        let loss = state.loss_logs.get(&record.root_id)?.get(index)?;
        Some(read(&loss.hli_path, loss.fidelity))
    }

    /// Removes exactly the record at `ctx_id`, if any (mirrors a successful
    /// `al_end_action` releasing one context). A later recording at the same
    /// ID starts a brand-new record; this never affects any other ID.
    pub(crate) fn remove(&self, ctx_id: ContextId) {
        let mut state = self.state.lock().unwrap();
        state.entries.remove(&ctx_id);
        state.loss_logs.remove(&ctx_id);
    }

    /// Returns the cached `Arc<ConversionMap>` for `key`, cloning a live
    /// reference if one already exists, or calling `create` and caching the
    /// result otherwise. A map already unreferenced by every record (its
    /// cached `Weak` no longer upgrades) is treated as absent: `create` runs
    /// again and its result replaces the stale cache entry.
    ///
    /// Exposed beyond `record_root` so a seam can translate a path (e.g.
    /// `al_begin_global_action`'s `datapath`) against an already-known
    /// mismatch before any context exists yet to record.
    pub(crate) fn get_or_create_map(
        &self,
        key: MapCacheKey,
        create: impl FnOnce() -> ConversionMap,
    ) -> Arc<ConversionMap> {
        let mut state = self.state.lock().unwrap();
        if let Some(map) = state.maps.get(&key).and_then(Weak::upgrade) {
            return map;
        }
        let map = Arc::new(create());
        state.maps.insert(key, Arc::downgrade(&map));
        map
    }
}

/// The single process-wide context registry every seam shares (CONTEXT.md's
/// "context registry"). Lazily initialised on first use rather than a bare
/// `static` because `ContextRegistry::new` allocates a `Mutex`-guarded
/// `HashMap`, which is not const-constructible.
pub(crate) static REGISTRY: LazyLock<ContextRegistry> = LazyLock::new(ContextRegistry::new);

#[cfg(test)]
mod tests {
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

        registry.record_read_loss(6, "field".to_string(), Fidelity::Lossy);

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
    fn recording_a_child_under_an_id_with_no_live_conversion_record_fails_and_clears_any_stale_entry()
     {
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

        assert!(!registry.record_root(
            5,
            "p".to_string(),
            1,
            matching_key,
            DUMMY_DIRECTION,
            || {
                loads.set(loads.get() + 1);
                dummy_map()
            },
        ));

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

        registry.record_read_loss(5, "field/a".to_string(), Fidelity::Lossy);
        registry.record_read_loss(5, "field/b".to_string(), Fidelity::Unmappable);

        assert_eq!(registry.loss_count(5), 2);
    }

    #[test]
    fn loss_count_never_counts_an_exact_read() {
        let registry = ContextRegistry::new();
        assert!(record_dummy_root(&registry, 5, "root/path".to_string(), 1));

        registry.record_read_loss(5, "field/a".to_string(), Fidelity::Exact);

        assert_eq!(registry.loss_count(5), 0);
    }

    #[test]
    fn loss_count_resolves_a_child_context_to_its_root() {
        let registry = ContextRegistry::new();
        assert!(record_dummy_root(&registry, 5, "root/path".to_string(), 1));
        assert!(registry.record_child(6, 5, "root/path/aos(1)".to_string()));

        registry.record_read_loss(6, "field".to_string(), Fidelity::Lossy);

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

        registry.record_read_loss(5, "field/a".to_string(), Fidelity::Lossy);
        registry.record_read_loss(5, "field/b".to_string(), Fidelity::Unmappable);

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
        registry.record_read_loss(5, "field/a".to_string(), Fidelity::Lossy);

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
        registry.record_read_loss(5, "field/a".to_string(), Fidelity::Lossy);
        assert_eq!(registry.loss_count(5), 1);

        registry.remove(5);

        assert_eq!(registry.loss_count(5), 0);
        assert_eq!(registry.with_loss_at(5, 0, |_, _| ()), None);
    }
}
