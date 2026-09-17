# Issue #240: runtime-binding fallback accessor audit

The deterministic major-version-mismatch unit test seeds only the resolved
fallback state; it never opens IMAS-Core, resolves a symbol, or forwards a
call. It asserts non-null, NUL-terminated C strings from all four public
accessors.

```console
$ cargo test fallback_accessors_expose_non_null_documented_c_strings --lib
$ cargo mutants --all-features --file src/core/core_binding.rs \
    --re 'replace (const2str|err2str|get_al_version|get_dd_version) -> \*const c_char with Default::default\(\)' \
    --output /private/tmp/imas-mvdd-issue240-accessor-mutations
$ bash scripts/audit-rust-line-coverage.sh
```

The focused test passed. Cargo-mutants generated and caught the four named
accessor replacements, with no missed, timed-out, or unviable outcome. The
fresh scoped line audit measured deterministic runtime-binding policy at
102/110 lines (92.7%) and the aggregate at 2,642/2,927 lines (90.3%), clearing
the per-group 80% and aggregate 90% floors.

| Accessor | Documented mismatch fallback |
| --- | --- |
| `const2str(HDF5_BACKEND)` | `"HDF5_BACKEND"` |
| `err2str(BACKEND_ERR)` | `"BACKEND_ERR"` |
| `getALVersion()` | detected version `"3.22.0"` |
| `getDDVersion()` | `"!!DEPRECATED!!"` |
