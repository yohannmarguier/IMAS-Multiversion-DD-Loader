# Stamp discovery asks a context of its own when the caller's cannot answer

Stored-DD-version discovery reads `ids_properties/version_put/data_dictionary` through the context the seam has just opened. When the caller opened that context under any access mode other than `READ_OP`, discovery instead opens a **read-mode context of its own**, on the same pulse and the same occurrence, reads the stamp through it, closes it, and only then forwards the caller's own open. ADR 0007's presumption still applies to whatever that probe reports.

This corrects ADR 0016 decision 11, which claimed a write-mode open "registers and translates exactly as a read-mode open does". That was true of the shim's policy and false of the result, because of the backend.

## The defect this replaces

`HDF5EventsHandler::beginAction` initializes the *reader's* per-IDS HDF5 group only when the context is opened `READ_OP`. A `WRITE_OP` open — `al_begin_global_action` or `al_begin_slice_action`, `GLOBAL_OP` or `SLICE_OP` rangemode alike — initializes the *writer's* group and nothing else. `REPLACE_OP` initializes neither.

So the stamp read discovery issued through the caller's own write-mode context found no reader to read from. It did not fail: it came back `size == 0`, which the one read-outcome classifier reports as not-found, which `version_stamp::discover` maps to `StampOutcome::Unstamped`, which `decide_occurrence_registration` treats exactly as it treats a genuinely unstamped occurrence — it registers nothing and forgets the occurrence cache.

The consequence was that **every write or delete issued through a write-mode open of an existing, differently-stamped occurrence was a plain, untranslated forward**: no path translation, no value transformation, no refusal, no loss log. The entire write-path policy ADR 0016 specifies, and which the recording-stub suite proves thoroughly at the policy level, was unreachable from a real write-capable caller. The stub cannot catch this, because a mock has no reader/writer group split to model; only issue #133's on-disk oracle surfaced it.

There was no ABI call sequence that avoided it. A write through a `READ_OP` context — which does discover correctly — fails with `HDF5Backend: unexpected value for gid in HDF5Writer::write_ND_Data()`, because the writer's group was never initialized for it. Opening twice, `READ_OP` to discover and then `WRITE_OP` to write, fails too: the second open's own discovery independently reports unstamped, and `StampOutcome::Unstamped`'s `OccurrenceCacheEffect::Forget` erases what the first open cached.

## Decisions

1. **The probe is keyed on the caller's access mode being other than `READ_OP`, not on it being `WRITE_OP`.** `READ_OP` is the one access mode under which a backend is obliged to have a reader for the context it just opened; every other mode is a mode whose context may have no reader. Enumerating the modes that do not — `WRITE_OP` today, `REPLACE_OP` as well, whatever a later IMAS-Core adds — would be a list to keep in step with an ABI this project does not own. The cost of an unnecessary probe is one open and close; the cost of a missing one is silent unconverted writes, which is the defect above.

2. **The probe runs before the caller's own open, not after their open fails to answer.** This is the ordering constraint, and it is the reason the fix is not simply "try the caller's context first and fall back". `HDF5Reader::close_file_handler` closes the pulse's per-IDS file handle and sets the shared `opened_IDS_files` entry to `-1`. A probe closed *after* the caller's write context was opened would therefore pull that handle out from under a context still holding a group id into it. Probing first means the caller's own open re-establishes the handle for itself, and the probe is invisible to it.

   The price is that the probe happens even when the caller's own open then fails, and once per occurrence open rather than once per occurrence. A `put_slice` loop pays one extra open/read/close per slice. That is accepted rather than optimised: the obvious optimisation — reuse the occurrence cache's remembered stored version instead of re-probing — would make the cache an authority for *registration* when today it is only an authority for translating a later `datapath`, and would need its own decision about what a `Forget` then means.

3. **The probe opens and closes through the plugin call family.** IMAS-Core's `al_begin_global_action` is `al_plugin_begin_global_action` plus `LLplugin::register_core_plugins` and plugin binding; `al_end_action` is `al_plugin_end_action` plus `LLplugin::endActionPlugin`. The plugin pair is therefore the plugin-free primitive, and it is what a context no HLI will ever see should use: registering and binding plugins for it is work with no consumer, and a probe issued from inside a plugin callback must not re-enter the plugin machinery that called it. The two are a matched open/close pair, so this obeys the same family rule every other seam in the interposition layer does.

   This is also why the probe takes no `CallFamily` parameter from its caller, and why `interpose::open_occurrence` did not have to grow one. The family the probe uses is a property of the probe, not of the seam that triggered it.

4. **The probe is always a global action, whichever seam triggered it, and all three occurrence-opening seams trigger one.** The stamp is a non-timed `STR_0D`; a slice or time-range context is the wrong shape to ask for it and would drag interpolation and time arguments into a question that has nothing to do with time. `al_begin_slice_action` and `al_begin_timerange_action` therefore trigger a *global* read-mode probe, exactly as `al_begin_global_action` does.

   Issue #136 names only the global and slice seams, because those are the two it was found through. `al_begin_timerange_action` is included anyway: all three share one adapter (`interpose::open_occurrence`), all three take an `rwmode`, and the defect is a property of the access mode rather than of the seam. Leaving time-range out would have meant a deliberate hole behind a shared function, which is harder to see than the fix.

5. **Every probe failure is `StampOutcome::Unstamped`.** A probe whose open fails — a backend that refuses a read-mode open, an occurrence that does not exist yet, which is the ordinary case for a writer — says nothing about a stored DD version. ADR 0007 already presumes a match in exactly that situation, and this adds no new presumption: it only stops the *absence of a reader* from being mistaken for the absence of a stamp.

6. **A probe that will not close is a leak, and the caller's open still succeeds.** `al_plugin_end_action`'s status is discarded. The caller asked to open an occurrence, not to clean up after the shim's own bookkeeping, so failing their open because the probe would not close turns one leaked IMAS-Core context into a denied open — and denies it for a reason the caller can neither see nor act on. Under a `put_slice` loop the leak is once per slice, which is the sharpest form of this cost and still the better side of the trade.

7. **The caller's own context is no longer read for the stamp when a probe ran.** Under a non-`READ_OP` open the probe is the single source, rather than a fallback consulted after the caller's context has been asked. Asking twice would cost a read that cannot succeed on this backend and could only disagree with the probe by being wrong.

## What this does not change

Nothing about a `READ_OP` open. The caller's own context is still the stamp reader for it, with the same injected unconverted read, the same classifier and the same registration rule. No stub-observable behaviour changes either, and that is structural rather than lucky: every scenario in the recording-stub suite that opens an occurrence with a latched HLI DD version opens it `READ_OP`, so no probe fires anywhere in that suite.

`al_begin_global_action`'s `datapath` still translates only from a previously cached mismatch, and still forwards unchanged on an occurrence's first use. The probe now knows the stored version before the caller's open is forwarded, so translating it on first use for a write-mode open has become *possible* — and is deliberately not done, because it would make `datapath` behave differently depending on the caller's access mode for no stated benefit. The second open of the same occurrence translates it as it always did.

## Consequences

- ADR 0016 decision 11's claim now holds, for the reason this ADR gives rather than the reason that ADR gave. The stamp still decides whether conversion applies and the access mode still does not; what changed is which context is asked.
- Discovery now depends on being able to *open a second context on the same pulse*. That is a stronger requirement than reading through one, and a backend that forbids it degrades to ADR 0007's presumption rather than failing. HDF5 permits it; the other five backends are unproven here, as they are everywhere else in this project.
- The write path's on-disk proofs became possible at all: `tests/real_core/write_delete_oracle_test.c` proves four of issue #133's five claims because of this change, and each of its nine scenarios fails if the probe is disabled.
- The same "read through the caller's own context" shape survives in one other place: `interpose::delete_candidates` probes each fan-out candidate for presence that way, so under a write-mode open it finds nothing, forwards no delete, and reports success. That is issue #138; this ADR fixes discovery, not every read the shim issues on its own behalf. Its ordering constraint does not transfer there — a fan-out runs mid-call, with the caller's context already open, so decision 2's "probe first" is not available to it.

## Considered Options

- **Key the probe on `rwmode == WRITE_OP`** — rejected in decision 1. It is a list of the ABI's non-read modes, maintained here, that is silently wrong the moment IMAS-Core adds to it.
- **Ask the caller's own context first and probe only on not-found** — rejected in decisions 2 and 6. It cannot be ordered safely, because a probe closed after the caller's open exists breaks that open's file handle, and on the only backend where any of this is observable the first read can never succeed anyway.
- **Read the stamp from the occurrence cache instead of probing again** — rejected in decision 2 as an optimisation that changes what the cache is for.
- **Refuse a write-mode open of an occurrence whose stamp cannot be read** — rejected. It converts a silent wrong answer into a loud wrong answer: a fresh occurrence has no stamp to read and a full `put` into one is the single most ordinary thing a writer does, so this would refuse the untouched-by-design workflow ADR 0016 explicitly keeps working.
