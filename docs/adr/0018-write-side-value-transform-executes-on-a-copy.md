# The write-side value transformation executes on a copy the policy owns

ADR 0010 settled how a value transformation executes on the read path: in place, on the buffer IMAS-Core returned, which the shim is free to modify because the HLI has not seen it yet. The write path cannot inherit that, because the buffer belongs to the caller. This ADR settles where the transformed bytes live, who owns them, and what happens at IMAS-Core's own shape gate. It is a decision record only: no code implements it yet.

## Decisions

1. **The transformation runs on a copy, and the caller's buffer is never modified.** `al_write_data`'s `data` is non-`const` but it is a source, and it is the HLI's own IDS field. Flipping it in place would negate the caller's live data as a side effect of storing it. The existing suite already asserts that a *refused* write leaves `data` and `size` untouched; a successful one must not do less.

2. **The seam policy owns the copy.** `run_write` receives a read-only `SourceView<'a>` — the mirror of the read path's `DataView`, with `Double(&'a [f64])`, `NotDouble` and `InvalidShape(&'static str)` — allocates a `Vec<f64>`, transforms it, and returns it in the verdict. The interposition layer borrows it for exactly one IMAS-Core call. The verdict's buffer field is an `Option`, where `None` means "forward the caller's buffer untouched".

   This keeps transformation *execution* in one module, matching the read path, and it does not breach ADR 0015: a `Vec<f64>` returned as a value is not a pointer handed across a boundary, and the policy still touches no raw pointer.

3. **The write path must compute the buffer length itself.** The read path never allocated, so it never needed to. A write needs the element count from `dim`, `size[0..dim]` and `datatype`, for rank up to `MAXDIM = 7`, before it can copy anything. This is the one piece of genuinely new machinery the write path adds, and it is where a rank or extent mistake would corrupt memory rather than merely return the wrong answer.

4. **A rank-0 value equal to IMAS-Core's empty sentinel forwards unchanged, with no transformation and no loss log entry.** IMAS-Core does not always write what it is given: `al_write_data` consults `Lowlevel::data_has_non_zero_shape` first and, for `dim == 0`, compares the scalar against a sentinel — `EMPTY_INT = -999999999`, `EMPTY_DOUBLE = -9.0E40`, `EMPTY_COMPLEX = (-9.0E40, -9.0E40)`. If it matches, `al_write_data` returns `code == 0` and stores nothing.

   `EMPTY_DOUBLE` is negative, so a sign flip crosses that gate. An HLI writes an unset COCOS-sensitive scalar as `-9.0E40`; a naive write-side flip sends `+9.0E40`; the gate then returns true and IMAS-Core stores `9.0E40` as a real measurement. The reverse direction would need a stored value of exactly `+9.0E40`, which is not physical, so only one direction of this is reachable — and it is reachable through ordinary use, because an HLI writes the sentinel for every field the caller left unset.

   The sentinel means "the caller did not set this". A transformation of an unset value has no meaning, so the shim forwards it and IMAS-Core keeps its existing behaviour of skipping the write. This is **not** a refusal: nothing failed and nothing was lost. It is the one place ADR 0016 decision 1's "never return `code == 0` for data we did not store" does not apply, and only because IMAS-Core would not have stored anything either.

5. **The scope of decision 4 is the scalar, because that is the scope of IMAS-Core's gate.** For rank above 0, `data_has_non_zero_shape` only checks that no extent is zero — and skips even that for `CHAR_DATA` above rank 1. So a sentinel-valued *element inside an array* passes the gate, and a transformation would corrupt it exactly as described above. No IMAS-Core gate catches that case and this ADR does not guard it: guarding would mean inspecting every element of every array write to decide whether it is really unset, which is a cost on every write for a case no HLI is known to produce. It is recorded as a limitation instead.

## Considered Options

- **Transform in place and restore afterwards** — rejected. It is observable by anything else holding that pointer, it is not exception-safe, and it makes a successful write briefly corrupt the caller's IDS.
- **Have the interposition layer own the copy** — rejected. It would split transformation execution across two modules, and the read path already establishes that the policy is where a transformation is applied.
- **Refuse a write whose value equals the sentinel** — rejected. An HLI writes the sentinel for every unset field, so this would fail an ordinary `put` for doing something entirely normal.
- **Do nothing about the sentinel** — rejected. It stores a fabricated `9.0E40` with `code == 0`, which is precisely the undetectable failure mode ADR 0016 decision 3 refuses to accept anywhere else.
- **Guard sentinel-valued array elements too** — rejected on cost, and recorded as a limitation in decision 5 rather than left unsaid.

## Consequences

- **The write path allocates, and the read path does not.** The allocation is bounded by one field's element count and lives for one IMAS-Core call, but it is the first allocation on a data path in this shim, and a rank-7 write of doubles is where its size is worth checking.
- **`ValueTransformation` gains a direction and an `inverse()`** (ADR 0016 decision 7), so this module's single `SignFlip` stops being self-inverting by accident and starts being self-inverting by declaration.
- **IMAS-Core's silent-skip behaviour is now part of the shim's contract surface.** Decision 4 depends on it, so a future IMAS-Core that stores the sentinel instead of skipping it would change what a write of an unset field means. Worth a note wherever the IMAS-Core pin moves.
- **A round trip cannot prove any of this.** Write flips HLI→stored and read flips stored→HLI, so the caller's own value comes back regardless of the sign on disk. Only a native read of the file proves the transformation happened and happened once, which is why ADR 0016's consequences make the native oracle primary.
