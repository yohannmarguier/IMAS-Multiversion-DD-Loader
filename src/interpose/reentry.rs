use std::cell::Cell;

thread_local! {
    /// How many guarded shim seams this thread is currently inside (ADR 0014).
    /// Only ever read through [`ReentryGuard`]; a thread-local rather than a
    /// global because the depth describes one call stack, and ADR 0003 already
    /// puts concurrent use of a single IMAS-Core context out of scope.
    static SHIM_REENTRY_DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Raises the thread's shim-seam depth for as long as a guarded seam is on the
/// stack, so a call that arrives *underneath* an in-flight IMAS-Core call can
/// recognise itself as reentrant (ADR 0014). The guard wraps the forwarded
/// call too, not just any conversion policy around it — the reentrant call
/// happens inside that call.
pub(super) struct ReentryGuard;

impl ReentryGuard {
    /// Enters a guarded seam, reporting whether one was already in flight on this
    /// thread.
    pub(super) fn enter() -> (Self, bool) {
        let already_entered = SHIM_REENTRY_DEPTH.with(|depth| {
            let entered = depth.get();
            depth.set(entered + 1);
            entered > 0
        });
        (Self, already_entered)
    }
}

impl Drop for ReentryGuard {
    fn drop(&mut self) {
        SHIM_REENTRY_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}
