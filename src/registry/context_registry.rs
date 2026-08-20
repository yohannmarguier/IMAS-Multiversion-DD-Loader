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

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, Weak};

use crate::conversion::conversion_map::{ConversionMap, Direction, Fidelity};
use crate::version::dd_version::DdVersion;

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
    ///
    /// Nothing outside this module's tests reads it: conversion resolves a
    /// child to its root in one lookup through `root_id` (issue #65), never by
    /// walking ancestry. It is retained because those tests are the only place
    /// the registry's parentage rules are pinned — that a child records the
    /// parent it was actually opened under, that a root has none, and that a
    /// recycled parent ID does not retroactively re-parent an existing child.
    /// Dropping the field would mean dropping those assertions.
    #[allow(
        dead_code,
        reason = "read by this module's tests, which pin the parentage rules"
    )]
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
    ///
    /// Only this module's tests call it, as the one way to ask from outside
    /// whether an ID is a live pulse context without going through a seam.
    /// The seams themselves never need to ask: each already holds the pulse
    /// context it was called with.
    #[allow(dead_code, reason = "the tests' only handle on data-entry entries")]
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

    /// Appends a non-exact read outcome directly to the loss log belonging to
    /// the root captured in a read's conversion-record snapshot. Exact reads
    /// never enter the log; if that root has ended, its log is gone and the
    /// outcome is deliberately not retained.
    ///
    /// This deliberately does not resolve a live `ctx_id`: a read must not
    /// attribute its outcome to a child context that was ended or recycled
    /// while IMAS-Core was running.
    pub(crate) fn record_read_loss_at_root(
        &self,
        root_id: ContextId,
        hli_path: String,
        fidelity: Fidelity,
    ) {
        if fidelity == Fidelity::Exact {
            return;
        }
        let mut state = self.state.lock().unwrap();
        if let Some(losses) = state.loss_logs.get_mut(&root_id) {
            losses.push(ReadLoss { hli_path, fidelity });
        }
    }

    /// Returns the number of loss-log entries retained for `ctx_id`'s root
    /// context (ADR 0012), resolving a child to its root exactly as
    /// loss recording does. Reports `0` — never a refusal — for a
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
    /// root context, in the order loss recording appended them, or
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
#[path = "tests/context_registry.rs"]
mod tests;
