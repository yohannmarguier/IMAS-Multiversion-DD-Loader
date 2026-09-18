//! ADR 0014's depth gate.
//!
//! IMAS-Core calls back into this shim while a seam is still in flight, and
//! by then the path in flight is already a *stored* path — resolving it again
//! would translate it twice. [`ReentryGuard`] is the one counter that tells a
//! seam it arrived underneath another, across every family Core can reenter.

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

#[cfg(test)]
mod tests {
    use super::ReentryGuard;

    #[test]
    fn callback_scope_preserves_the_outer_entry_then_restores_a_fresh_entry() {
        let (outer, outer_is_reentrant) = ReentryGuard::enter();
        assert!(
            !outer_is_reentrant,
            "the first entry receives conversion policy"
        );

        {
            let (_callback, callback_is_reentrant) = ReentryGuard::enter();
            assert!(callback_is_reentrant, "a nested callback passes through");
        }

        let (after_callback, after_callback_is_reentrant) = ReentryGuard::enter();
        assert!(
            after_callback_is_reentrant,
            "leaving the callback preserves the in-flight outer entry"
        );
        drop(after_callback);
        drop(outer);

        let (_fresh, fresh_is_reentrant) = ReentryGuard::enter();
        assert!(
            !fresh_is_reentrant,
            "leaving the outer entry restores conversion for the next call"
        );
    }
}
