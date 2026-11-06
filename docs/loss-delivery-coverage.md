# Loss exports and file delivery coverage

Issue #255 measures the public loss API and process-local file writer with
the 22 CTest scenarios labelled `loss-adapter` in `cmake/tests/Shim.cmake`:

```console
bash scripts/collect-rust-integration-coverage.sh recording-stub
```

Existing C scenarios supply successful non-exact reads, pointer/index errors,
untracked contexts, lazy creation, filename collisions, default and disabled
destinations, and recoverable file failures. They run in separate processes,
so every writer initializes independently. The focused report uses the private
XML regression library; it does not claim to measure graph acquisition.

Two additional public-ABI scenarios close the delivery gaps:

- `loss-delivery-operations-survive-close` checks read absence, primary-only
  candidate writes, delete fan-out, and refused reads/writes/deletes. It checks
  the exact operation, fidelity and caller/stored path through root and child
  exports, then every complete file row including URI, IDS and ordered DD
  versions. Negative, zero, one-byte-short and exact-fit path buffers preserve
  untouched outputs or the trailing NUL as appropriate. Closing the root and
  child clears their queryable losses and leaves file contents unchanged.
- `loss-delivery-concurrent-first-loss` releases eight threads together and
  performs 128 refused writes involving two distinct loss keys. All 128 entries
  reach the context log, while each complete row appears exactly once in the
  sole process file. Refusals never call the recording Core, avoiding races in
  that stub's non-concurrent recording fields. No clock boundary is assumed.

On the November 6 implementation run (Darwin, Rust 1.97.1,
cargo-llvm-cov 0.9.1), the focused run measured:

| Source | Covered / executable lines |
| --- | ---: |
| `src/interpose/loss.rs` | 89 / 89 |
| `src/loss_file.rs` | 179 / 196 |

The remaining writer lines handle a pre-epoch clock, unavailable working
directory, preamble-write failure, file-create errors after directory validation,
and exhaustion of all filename suffixes. The existing injected writer unit
tests cover these recoverable outcomes; this ABI suite does not manipulate the
host clock or invent new filesystem-error semantics. Exact uncovered lines and
the measured revision are retained in `loss-adapter.json`.

Fresh-build mutations were executed with the affected C executable deleted
before each rebuild:

| Mutation | Observed failure |
| --- | --- |
| suppress the exported fidelity value | the mixed-operation scenario rejects its verdict |
| bypass file delivery while retaining context entries | the mixed-operation scenario finds no file |
| append repeated keys instead of deduplicating | the concurrent scenario finds a duplicate complete row |

All mutations were restored and both new scenarios passed after rebuilding.
The collector retains failed-run evidence and returns failure if either the
behavioral ledger or either source measurement fails. These are integration
measurements, not a whole-crate percentage gate.
