# IMAS-Core Functionality Inventory

**Status:** Draft v2 · 2026-07-03 (reviewed by an independent pass; see
correction notes inline and the updated Summary at the end)
**Purpose:** A precise, per-capability inventory of what IMAS-Core currently does,
for use as Phase 0 characterization input to the migration described in
`NORTH_STAR.md` (performance + DD-version-agnosticism). This is a working
engineering reference, not published documentation — see `docs/adr/0001-*.md`
for why it's organized this way, and `CONTEXT.md` for the vocabulary used
throughout (**Capability**, **Audience**, **Dependency**, **Limit**,
**Operational characteristic**).

**Structure:** one section per audience — **User** (HLI implementer, via the
public C ABI), **Backend implementer** (via the `Backend` C++ contract), and
**Plugin author** (via the plugin C++ interfaces). Kept as one growing file
rather than split per audience, since they share the vocabulary/hard-bounds
table below and cross-reference each other constantly (e.g. a Backend's
`list_filled_paths` support directly determines the User capability's
Limits).

**Scope:** this pass covers the `al_*` C-ABI functions and `al_const.h`
(Part 1), the `Backend` abstract class (Part 2), and the plugin interface
hierarchy (Part 3) — the three audiences named in `CONTEXT.md`. It
deliberately does **not** yet cover several other public headers
(`PUBLIC_HEADER_FILES` in `CMakeLists.txt`): `al_context.h` (the
`Context`/`OperationContext`/`DataEntryContext`/`ArraystructContext` classes
that every Backend method actually receives), `al_exception.h` (the
exception hierarchy backing "throws" in Part 2/3), `uri_parser.h`,
`data_interpolation.h`, and `access_layer_plugin_manager.h`. It also doesn't
scope the exported C++ classes inside `al_lowlevel.h` itself (`Lowlevel`,
`LLplugin`, `LLenv`) that a C++ HLI could call directly instead of the
`extern "C"` functions. These are real gaps, not silent omissions — see
"Open items" at the end of each Part.

**Method:** every signature below is copied verbatim from the relevant header
on `develop` @ `ad0f2e2910dac8dfd552e1f8ec67e8b4ac70162c` (cross-checked
against `upstream/develop`), cross-referenced against actual implementation
bodies and, where relevant, the compiled symbol table (`nm -gU` on
`libal.dylib`) — not just read from headers/doc comments in isolation. See
the `al_plugin_begin_timerange_action` case study and the
`list_filled_paths` per-backend matrix below for why that distinction
matters: header doc comments in this codebase are sometimes stale or
imprecise relative to the actual implementation.

---

# Part 1 — User (HLI implementer) audience

The public C ABI in `include/al_lowlevel.h` (+ `al_const.h`) as called by an
HLI implementer (Python, Fortran, Java, MATLAB, C++).

---

## Shared vocabulary (needed to call anything below)

Every User capability is parameterized by a small set of constants defined in
`al_const.h`. These aren't capabilities themselves — they're the argument
domain.

| Group | Values |
|---|---|
| `BACKEND` (backend selection, used indirectly via URI) | `NO_BACKEND`, `ASCII_BACKEND`, `MDSPLUS_BACKEND`, `HDF5_BACKEND`, `MEMORY_BACKEND`, `UDA_BACKEND`, `FLEXBUFFERS_BACKEND` |
| Operation range (`al_begin_*_action`'s implicit op kind) | `GLOBAL_OP`, `SLICE_OP`, `TIMERANGE_OP` |
| Access mode (`rwmode`) | `READ_OP`, `WRITE_OP`, `REPLACE_OP` (slice only) |
| Interpolation mode (`interpmode`) | `UNDEFINED_INTERP`, `CLOSEST_INTERP`, `PREVIOUS_INTERP`, `LINEAR_INTERP` |
| Pulse open/close mode | `OPEN_PULSE`, `FORCE_OPEN_PULSE`, `CREATE_PULSE`, `FORCE_CREATE_PULSE`, `CLOSE_PULSE`, `ERASE_PULSE` |
| Data type (`datatype`) | `CHAR_DATA`, `INTEGER_DATA`, `DOUBLE_DATA`, `COMPLEX_DATA` — **note: no boolean type, no single-precision float** |
| Serializer protocol | `ASCII_SERIALIZER_PROTOCOL`, `FLEXBUFFERS_SERIALIZER_PROTOCOL`, `DEFAULT_SERIALIZER_PROTOCOL` (alias of `FLEXBUFFERS_SERIALIZER_PROTOCOL`) |
| Time sentinel | `UNDEFINED_TIME` — passed as `time` to `al_begin_slice_action` to mean "append/replace the last slice" |

**Hard bounds** (from `include/al_defs.h.in`):
- `MAXDIM = 7` — maximum array rank/dimension for any `dim` argument
- `MAX_ERR_MSG_LEN = 256` — max length of `al_status_t.message`

**A note on `const2str`/`err2str` coverage** (see Introspection/diagnostics
cluster below): `alconst::constmap` in `al_const.h` does **not** include every
value in the table above — `TIMERANGE_OP` and `FLEXBUFFERS_BACKEND` are
defined as constants but absent from the map. Calling `const2str()` on them
doesn't error; it silently returns `""` (verified in `src/al_const.cpp:4-11`).

All functions return `al_status_t { int code; char message[MAX_ERR_MSG_LEN]; }`
— `code == 0` is success, `code < 0` is failure with `message` populated.

---

## Cluster 1 — Pulse lifecycle

Opening/closing a data entry, and inspecting any context.

### `al_begin_dataentry_action`
```c
al_status_t al_begin_dataentry_action(const char *uri, int mode, int *dectxID);
```
C++ callers also get a `std::string` overload (same name, C++ linkage only,
declared outside the `extern "C"` block — `al_lowlevel.h:547`).

- **Contract:** opens an IMAS data entry addressed by `uri`. `mode` is one of
  `OPEN_PULSE` / `FORCE_OPEN_PULSE` / `CREATE_PULSE` / `FORCE_CREATE_PULSE`.
  Returns a pulse context id in `*dectxID` (0 = null context).
- **Dependencies:** must be the first call in any session; every Cluster 2
  data-access capability in this document takes a context that traces back
  to this one. (Cluster 3 plugin registration/binding/parameter calls and
  Cluster 4 introspection calls do not — they take no context at all.) URI
  must be parseable (`include/uri_parser.h`) and resolve to a backend that's
  actually compiled in (MDSplus/UDA are optional, off by default).
- **Limits:** none documented at this level; backend-specific URI support
  varies (see the separate backends guide, `docs/source/user_guide/backends_guide.rst`).

### `al_close_pulse`
```c
al_status_t al_close_pulse(int pulseCtx, int mode);
```
- **Contract:** closes (`CLOSE_PULSE`) or closes-and-removes (`ERASE_PULSE`)
  the pulse identified by `pulseCtx`.
- **Dependencies:** `pulseCtx` from `al_begin_dataentry_action`.
- **Limits:** none documented.

### `al_context_info`
```c
al_status_t al_context_info(int ctx, char **info);
```
- **Contract:** returns a string describing any context (pulse, operation, or
  arraystruct — `ctx` can be any of the three).
- **Dependencies:** `ctx` must be a currently-open context of any kind.
- **Limits:** **caller must free `*info`** — the header comment says so
  explicitly ("NEED TO BE FREEED!!"). Not doing so leaks.

### `al_get_backendID`
```c
al_status_t al_get_backendID(int ctx, int *beid);
```
- **Contract:** returns which `BACKEND` enum value is active for `ctx` (any
  context type, same as `al_context_info`).
- **Dependencies:** `ctx` must be open.
- **Limits:** none documented.

### `al_build_uri_from_legacy_parameters`
```c
al_status_t al_build_uri_from_legacy_parameters(const int backendID, const int pulse, const int run,
    const char *user, const char *tokamak, const char *version, const char *options, char** uri);
```
C++ overload with `std::string` in/out params also exists (`al_lowlevel.h:548-555`).
- **Contract:** constructs a URI string from the pre-URI legacy parameter set
  (backend id, pulse/run numbers, user, tokamak, version, options) — a
  migration aid for callers still using the old parameter style.
- **Dependencies:** none beyond valid parameter values.
- **Limits:** not verified whether `*uri` needs caller-side freeing like
  `al_context_info`'s `*info` — the doc comment doesn't say either way. Flag
  this for confirmation during reimplementation rather than assuming.

---

## Cluster 2 — Data access

The actual read/write/delete/iterate operations. This is the largest cluster
and the one `NORTH_STAR.md` identifies as the performance bottleneck (§2.3,
§7 — "the node-by-node `Backend` contract is the ceiling").

### `al_begin_global_action`
```c
al_status_t al_begin_global_action(int pctxID, const char* dataobjectname, const char* datapath, int rwmode, int *octxID);
```
- **Contract:** starts a read/write operation on an entire DATAOBJECT (IDS).
  `datapath` is documented as "path to data node for partial get operation."
- **Dependencies:** `pctxID` from `al_begin_dataentry_action`.
- **Limits:** **`datapath` mostly doesn't do what its own doc comment
  implies, with one exception.** Per `NORTH_STAR.md` §2.3.6, HDF5, MDSplus,
  Memory, ASCII, and Flexbuffers all ignore it for I/O restriction (verified:
  zero references to `datapath` in any of these backends). **UDA is the
  exception**: in remote mode with `cache_mode=ids`, `UDABackend` reads
  `op_ctx->getDatapath()` and uses it to scope which paths `populate_cache()`
  requests from the server (`src/uda/uda_backend.cpp:1017-1031`). So partial
  get via `datapath` exists today, narrowly, only through UDA's remote cache
  path — not as a general capability of the core. True general partial get
  below the HLI is still a Phase 2 target (`al_read_subtree`).

### `al_begin_slice_action`
```c
al_status_t al_begin_slice_action(int pctxID, const char* dataobjectname, int rwmode, double time, int interpmode, int *octxID);
```
- **Contract:** starts an operation on one time slice of a DATAOBJECT.
  `rwmode` may additionally be `REPLACE_OP` here (global doesn't support
  replace). `time = UNDEFINED_TIME` to append/replace the last slice.
- **Dependencies:** `pctxID` open; backend must support the requested
  `interpmode`. This is not universal: Memory, ASCII, and Flexbuffers report
  `supportsTimeDataInterpolation() == false` and their
  `initDataInterpolationComponent()` throws "...does not support time range
  and time slices operations" (`src/ascii_backend.h:100`,
  `src/memory_backend.h:682`, `src/flexbuffers_backend.h:57`) — see Part 2,
  Cluster E for the full matrix.
- **Operational characteristics:** for the HDF5 backend, slice mode disables
  dataset buffering (`src/hdf5/hdf5_dataset_handler.cpp:733`), costing ~2
  `H5Dread` calls (data + `_SHAPE`) per field per AOS element — this is a
  known, currently-unoptimized hot path (`NORTH_STAR.md` §7, P3).

### `al_begin_timerange_action`
```c
al_status_t al_begin_timerange_action(int pctxID, const char* dataobjectname, int rwmode,
    double tmin, double tmax, const double* dtime_buffer, const int* dtime_shape, int interpmode, int *octxID);
```
- **Contract:** starts an operation over a time range `[tmin, tmax]`. If
  `dtime_buffer`/`*dtime_shape` (element count, **not** a data value — see
  the header's `double*` vs. this function's `int*` for the same-named
  parameter, a known source of confusion, ADR-worthy if it recurs) is
  non-empty, data is resampled onto that homogeneous time vector.
- **Dependencies:** backend must declare `supportsTimeRangeOperation() ==
  true` (a Backend implementer capability-negotiation flag, `al_backend.h`)
  — otherwise this throws internally with "Selected backend does not support
  time range operations."
- **Limits:** verified per-backend (see Part 2, Cluster E): only **HDF5**
  (`true`) supports this unconditionally. **MDSplus explicitly returns
  `false`** despite supporting time *data interpolation* — the two
  capabilities are independent, not implied by each other. **UDA is
  conditional, not a simple delegation**: in local-access mode it delegates
  to the local backend; in remote mode it instead queries the UDA server
  plugin's reported version and returns `true` only if `version > 1.4.0`
  (`src/uda/uda_backend.cpp:1189-1223`) — so UDA support depends on the
  remote server's plugin version, not just on which local backend it might
  proxy to. Memory, ASCII, and Flexbuffers all return `false`. So
  `al_begin_timerange_action` reliably works only against HDF5 today; UDA
  support is version-gated on the server side.

### `al_begin_arraystruct_action`
```c
al_status_t al_begin_arraystruct_action(int ctxID, const char *path, const char *timebase, int *size, int *actxID);
```
- **Contract:** starts an operation on an array of structures (AOS), either
  top-level (from an operation context) or nested (from another arraystruct
  context — `ctxID` accepts either). `size` is in/out: caller supplies the
  size when writing, backend reports it when reading.
- **Dependencies:** `ctxID` open (operation or arraystruct).
- **Limits:** none documented beyond the general `MAXDIM` bound on the data
  eventually read/written under this AOS.

### `al_end_action`
```c
al_status_t al_end_action(int ctxID);
```
- **Contract:** ends any action — pulse, operation, or arraystruct context —
  making `ctxID` invalid afterward.
- **Dependencies:** `ctxID` must be a currently-open context of any kind.
- **Limits:** none documented.

### `al_read_data` / `al_write_data`
```c
al_status_t al_read_data(int ctxID, const char *field, const char *timebase, void **data, int datatype, int dim, int *size);
al_status_t al_write_data(int ctxID, const char *field, const char *timebase, void *data, int datatype, int dim, int *size);
```
- **Contract:** reads/writes one field (scalar or array up to `MAXDIM` rank)
  at `field`, relative to `ctxID` (absolute if prefixed `/`). `timebase` names
  the associated time-base field. `read_data` returns `0` in `al_status_t`
  region — note the doc also separately says "result returns 0 when there is
  no such data (or 1 on success)" for the *inner* return value in the
  `Backend::readData` contract; the two "0" meanings (status code vs.
  found-data flag) are easy to conflate and worth being careful about when
  reimplementing.
- **Dependencies:** `ctxID` from a `begin_*_action` call; `datatype` must be
  one of the 4 supported types.
- **Limits:** `dim` ≤ `MAXDIM` (7). No boolean or single-precision float type.
- **Operational characteristics:** Python bindings: reads are zero-copy;
  writes copy twice (`astype` + `asfortranarray`,
  `python/imas_core/_al_lowlevel.pyx:615-631`) — a known, unaddressed
  asymmetry (`NORTH_STAR.md` §2.3.5).

### `al_delete_data`
```c
al_status_t al_delete_data(int ctx, const char *path);
```
- **Contract:** deletes the data at `path` — can be a single signal, a
  structure, or (if `path` is the DATAOBJECT root) the whole DATAOBJECT.
- **Dependencies:** `ctx` open.
- **Limits:** none documented.

### `al_iterate_over_arraystruct`
```c
al_status_t al_iterate_over_arraystruct(int aosctx, int step);
```
- **Contract:** advances the "current element" index within an AOS context by
  `step` (typically `1`).
- **Dependencies:** `aosctx` from `al_begin_arraystruct_action`.
- **Limits:** none documented (no explicit bounds-check behavior noted at
  this level — worth checking backend implementations directly).

### `al_get_occurrences`
```c
al_status_t al_get_occurrences(int pctxID, const char* ids_name, int** occurrences_list, int* size);
```
- **Contract:** lists which occurrence numbers of `ids_name` are non-empty in
  the backend.
- **Dependencies:** `pctxID` open.
- **Limits:** none documented; caller-side freeing of `*occurrences_list` not
  explicitly stated (same ambiguity flagged for `al_build_uri_from_legacy_parameters`).

### `al_list_filled_paths`
```c
al_status_t al_list_filled_paths(int pctxID, const char* dataobjectname, char*** path_list, int* size);
```
- **Contract:** lists DD paths (no indices) with filled data in the backend.
- **Dependencies:** `pctxID` open.
- **Limits:** the header doc comment says *"only implemented for tensorizing
  backends (i.e. the HDF5 backend)"* — verified against every backend's
  actual implementation, and it's more absolute than that phrasing suggests:

  | Backend | Behavior |
  |---|---|
  | HDF5 | real implementation (`H5Literate`-based) |
  | UDA | delegates to a local backend if configured, else queries the remote server |
  | MDSplus, Memory, ASCII, Flexbuffers | **throws `ALBackendException` unconditionally** — not partial, not empty, a hard failure |
  | NoBackend | throws if `verbose`, else silently returns an empty list |

  So calling this against 4 of 6 backends raises at runtime, it doesn't
  degrade gracefully. Also: paths may come back in any order, and **the
  caller must free the list and every string in it** (both stated explicitly
  in the header, unlike the ambiguous cases above).

---

## Cluster 3 — Plugin management

Registering, binding, and configuring a plugin from the User (HLI) side. The
plugin's *own* implementation is a separate audience (Plugin author) — see
Part 3. See `CONTEXT.md` for **Plugin registration** vs. **Plugin binding**
as distinct terms.

### `al_register_plugin` / `al_unregister_plugin`
```c
al_status_t al_register_plugin(const char *plugin_name);
al_status_t al_unregister_plugin(const char *plugin_name);
```
- **Contract:** `register` loads `$IMAS_AL_PLUGINS/<plugin_name>_plugin.so`
  via `dlopen`, resolves exported `create()`/`destroy()` C factory symbols,
  and instantiates the plugin. `unregister`'s actual contract is narrower
  than "destroys it and removes it from the registry" — see Limits.
- **Dependencies:**
  - Global gate: `IMAS_AL_ENABLE_PLUGINS` env var (checked on every read/write
    call, `src/al_lowlevel.cpp:108`) must be set for the plugins framework to
    be active at all.
  - Discovery: `IMAS_AL_PLUGINS` env var must point to a directory containing
    `<plugin_name>_plugin.so` (verified in `src/al_lowlevel.cpp:323-357`).
  - The `.so` must export `create()`/`destroy()` matching the
    `create_t`/`destroy_t` typedefs in `access_layer_base_plugin.h` — this is
    a build/packaging dependency on the Plugin author's side, not something
    the User controls, but it will fail at `register` time if unmet.
- **Limits:**
  - Registering an already-registered name, or unregistering an unregistered
    name, both throw (`ALLowlevelException`) rather than being no-ops.
  - **`al_unregister_plugin` only actually destroys/erases a plugin that is
    currently bound to at least one path.** `LLplugin::unregisterPlugin`
    (`src/al_lowlevel.cpp:382-431`) walks `boundPlugins` looking for the name,
    and the `destroy()` call + `llpluginsStore.erase()` both happen only
    inside that loop. A plugin that was registered but never bound is left
    in the registry, not destroyed, on unregister — despite the contract
    reading unconditionally. Re-registering that name later still throws
    "already registered."
  - **`dlopen` failure in `registerPlugin` does not throw.** A failed
    `dlopen()` (`src/al_lowlevel.cpp:350-357`) is reported via `printf` and
    an `assert(plugin_handler != nullptr)` — in an `NDEBUG` release build the
    assert is compiled out and execution continues with a null handle,
    heading toward a crash on the subsequent `dlsym` calls rather than a
    catchable exception.

### `al_bind_plugin` / `al_unbind_plugin`
```c
al_status_t al_bind_plugin(const char* fieldPath, const char* pluginName);
al_status_t al_unbind_plugin(const char* fieldPath, const char* pluginName);
```
- **Contract:** activates/deactivates a registered plugin for a specific DD
  node path. **A registered-but-unbound plugin is inert** — ignored during
  get/put (this is the distinction the framework docs sometimes blur; see
  `CONTEXT.md`).
- **Dependencies:** `pluginName` must already be registered via
  `al_register_plugin`.
- **Limits:** binding the same plugin twice to the same path throws
  (`src/al_lowlevel.cpp:269-273`); unbinding a plugin from a path it isn't
  bound to does not error — it `printf`s a message and silently no-ops
  (`src/al_lowlevel.cpp:298-299`).

### `al_bind_readback_plugins` / `al_unbind_readback_plugins`
```c
al_status_t al_bind_readback_plugins(int ctxid);
al_status_t al_unbind_readback_plugins(int ctxID);
```
- **Contract:** manages the association between a plugin and its declared
  *readback* plugin(s) — the plugin(s) responsible for reading back data this
  plugin wrote (e.g. decompressing it), per `readback_plugin_feature`.
- **Dependencies:** only meaningful for plugins that implement
  `readback_plugin_feature` (a Plugin author capability — see Part 3,
  Cluster 4).
- **Limits:** not fully traced in this pass.

### `al_is_plugin_registered`
```c
al_status_t al_is_plugin_registered(const char* pluginName, bool *is_registered);
```
- **Contract:** simple boolean query against the plugin registry.
- **Dependencies:** none beyond the plugins framework being enabled.
- **Limits:** none documented.

### `al_setvalue_parameter_plugin` / `al_setvalue_int_scalar_parameter_plugin` / `al_setvalue_double_scalar_parameter_plugin`
```c
al_status_t al_setvalue_parameter_plugin(const char* parameter_name, int datatype, int dim, int *size, void *data, const char* pluginName);
al_status_t al_setvalue_int_scalar_parameter_plugin(const char* parameter_name, int parameter_value, const char* pluginName);
al_status_t al_setvalue_double_scalar_parameter_plugin(const char* parameter_name, double parameter_value, const char* pluginName);
```
- **Contract:** three variants (generic typed, int-scalar convenience,
  double-scalar convenience) for configuring a named parameter on a
  registered plugin — reaches the Plugin author's `setParameter` method.
- **Dependencies:** `pluginName` must be registered.
- **Limits:**
  - **Calling any of these three with an unregistered `pluginName` crashes
    rather than erroring.** `LLplugin::setvalueParameterPlugin`
    (`src/al_lowlevel.cpp:453-457`) does `llpluginsStore[plugin_name]`,
    which for an unknown key default-constructs an `LLplugin` entry with
    `al_plugin == NULL`, then immediately calls `al_plugin->setParameter(...)`
    — a null-pointer dereference the surrounding try/catch cannot turn into
    an `al_status_t` failure. This is a real gap versus the registration
    check other Cluster 3 calls perform.
  - The generic variant's `dim`/`size` presumably inherit the same `MAXDIM`
    bound as `al_read_data`/`al_write_data`, but this isn't stated explicitly
    for plugin parameters — worth confirming.

### `al_write_plugins_metadata`
```c
al_status_t al_write_plugins_metadata(int ctxid);
```
- **Contract:** triggers writing provenance metadata (name, version, commit,
  etc. — from `provenance_plugin_feature`) for currently-bound plugins into
  the backend, associated with `ctxid`.
- **Dependencies:** `ctxid` open; only meaningful if plugins are bound.
- **Limits:** none documented.

---

## Cluster 4 — Introspection / diagnostics

```c
const char * const2str(int id);      // al_const.h
const char * err2str(int id);        // al_const.h
const char * getALVersion();         // al_const.h
const char * getDDVersion();         // al_const.h
```
- **Contract:**
  - `const2str(id)` — maps a constant value (from the vocabulary table above)
    to its symbolic name string.
  - `err2str(id)` — maps an error code (`UNKNOWN_ERR`, `CONTEXT_ERR`,
    `BACKEND_ERR`, `LOWLEVEL_ERR`) to its name string.
  - `getALVersion()` — returns the AL library version string.
  - `getDDVersion()` — returns the DD version string.
- **Dependencies:** none — these are pure lookups, no context required.
- **Limits:**
  - `const2str`/`err2str` **silently return `""` for unmapped ids** rather
    than erroring (`src/al_const.cpp:4-11`) — confirmed that `TIMERANGE_OP`
    and `FLEXBUFFERS_BACKEND` are defined constants but absent from
    `alconst::constmap`, so `const2str(TIMERANGE_OP)` returns `""`.
  - `getDDVersion()` **is intentionally non-functional** — per `CLAUDE.md`
    and `include/al_defs.h.in:57`, the compile-time `DD_VERSION` is
    deliberately set to `"!!DEPRECATED!!"`, and `python/tests/test_imasdef.py`
    asserts it stays that way. Calling this capability today returns a
    sentinel, not real version information — by design, not by bug.

---

## Open items — User audience

- Verify caller-side memory ownership for `al_build_uri_from_legacy_parameters`'s
  `*uri` and `al_get_occurrences`'s `*occurrences_list` (unlike
  `al_context_info` and `al_list_filled_paths`, the header doesn't say
  explicitly).
- Confirm whether plugin-parameter `dim`/`size` in
  `al_setvalue_parameter_plugin` is bound by `MAXDIM` like ordinary data.
- `al_context.h` (`Context`/`OperationContext`/`DataEntryContext`/
  `ArraystructContext`), `al_exception.h`, `uri_parser.h`,
  `data_interpolation.h`, and `access_layer_plugin_manager.h` are public
  headers not yet inventoried (see "Scope" note at the top of this
  document).
- The exported C++ classes inside `al_lowlevel.h` (`Lowlevel`, `LLplugin`,
  `LLenv`), callable directly by a C++ HLI instead of the `extern "C"`
  functions, are not yet scoped in or out.

---

# Part 2 — Backend implementer audience

Someone implementing the `Backend` abstract class (`include/al_backend.h`) to
add a new storage engine. Exactly one backend is active per data entry,
selected via `Backend::initBackend(DataEntryContext*)` (a static factory).
Six concrete backends exist today: HDF5, MDSplus, UDA, Memory, ASCII,
Flexbuffers (plus an internal `NoBackend` null-object).

All virtual methods below are pure virtual (`= 0`) unless noted — every
concrete backend must implement all of them, even if only to throw
"not implemented" (as several do — see Cluster D).

---

## Cluster A — Instantiation

### `initBackend`
```c++
static Backend* initBackend(DataEntryContext *ctx);
```
- **Contract:** factory method — given a `DataEntryContext` (which carries the
  resolved backend ID from the URI), constructs and returns the appropriate
  concrete `Backend` subclass.
- **Dependencies:** the requested backend must actually be compiled in —
  MDSplus and UDA are optional, off by default (`CLAUDE.md`); `al_backend.cpp:75`
  shows an explicit `#ifdef`-gated throw ("UDA backend is not available
  within current install") when a backend ID is requested but not built.
- **Operational characteristics:** immediately after construction, this
  factory conditionally calls `initDataInterpolationComponent()` on the new
  instance — see Cluster E, this is not something the Backend implementer
  triggers themselves.
- **Limits:** an unrecognized backend ID throws `ALBackendException("Wrong
  backend identifier ...")` — not a null return.

---

## Cluster B — Pulse lifecycle

### `getVersion`
```c++
virtual std::pair<int,int> getVersion(DataEntryContext *ctx) = 0;
```
- **Contract:** returns `<major,minor>`. `ctx == NULL` → the version of the
  *installed* backend; `ctx != NULL` → the version *stored in* that pulse
  file. Used to detect drift between the installed backend and the file it's
  about to write to.
- **Dependencies:** implementers must follow a specific versioning
  convention for the caller-side compatibility check to mean anything:
  *"non-backward compatible changes → bump major; non-forward compatible
  changes → bump minor"* (doc comment, `al_backend.h:44-52`).
- **Limits:** the compatibility check that consumes this actually lives in
  the `al_plugin_begin_global_action`/`al_plugin_begin_slice_action`/
  `al_plugin_begin_timerange_action` implementations
  (`src/al_lowlevel.cpp:955-966`, `:1075-1086`) — the User-facing
  `al_begin_*_action` wrappers delegate to these, they don't run the check
  themselves. It only fires **on write** (`switch(rwmode) { case write_op:
  ... }`) and only compares **minor** versions — a Backend implementer who
  gets the major/minor convention backwards would silently defeat this
  guard, not get an error about it.

### `openPulse` / `closePulse`
```c++
virtual void openPulse(DataEntryContext *ctx, int mode) = 0;   // OPEN_PULSE / FORCE_OPEN_PULSE / CREATE_PULSE / FORCE_CREATE_PULSE
virtual void closePulse(DataEntryContext *ctx, int mode) = 0;  // CLOSE_PULSE / ERASE_PULSE
```
- **Contract:** mirrors the User-facing `al_begin_dataentry_action`/
  `al_close_pulse` one level down.
- **Dependencies:** none beyond a valid `DataEntryContext`.
- **Limits:** both `@throw BackendException` per the doc comment — the
  Backend implementer's error-signaling contract is exceptions, not return
  codes, unlike `readData`'s int-return convention (see Cluster C).

---

## Cluster C — Operation & data I/O

### `beginAction` / `endAction`
```c++
virtual void beginAction(OperationContext *ctx) = 0;
virtual void endAction(Context *ctx) = 0;
```
- **Contract:** `beginAction` only ever takes an `OperationContext*`
  (global/slice/timerange, encoded in `ctx`); `endAction` takes the general
  `Context*` base — it must handle **any** of pulse/operation/arraystruct,
  dispatching on `ctx->getType()`.
- **Limits — a real gotcha for reimplementation:** the User-facing wrapper
  `al_end_action` (`al_lowlevel.cpp`) does more than call `endAction`: *if*
  `ctx->getType() == CTX_PULSE_TYPE`, it **also deletes the `Backend`
  instance itself** right after calling `endAction`. So ending a pulse-type
  context is effectively "this Backend object is about to be destroyed" —
  a Backend implementer's `endAction` must leave the object in a state safe
  to immediately destruct, and can't assume `closePulse` was called first
  (it's a separate, User-driven call).

### `writeData` / `readData`
```c++
virtual void writeData(Context *ctx, std::string fieldname, std::string timebasename, void* data, int datatype, int dim, int* size) = 0;
virtual int  readData(Context *ctx, std::string fieldname, std::string timebasename, void** data, int* datatype, int* dim, int* size) = 0;
```
- **Contract:** the actual storage-level read/write for one field.
- **Limits — an inverted-convention gotcha:** `readData` returns `int`, and
  per its own doc comment: *"returns 0 when there is no such data (or 1 on
  success)."* This is a **different meaning of `0`** than the User-facing
  `al_status_t.code` (where `0` means success). A Backend implementer must
  not conflate the two: `0` from `readData` is "not found" (not an error),
  while `0` from the eventual `al_status_t` is "success." Easy to get backwards
  when translating between the two layers during a rewrite.
- **Dependencies:** `dim` ≤ `MAXDIM` (7); `datatype` one of the 4 supported.

### `deleteData`
```c++
virtual void deleteData(OperationContext *ctx, std::string path) = 0;
```
- **Contract:** deletes the subtree at `path`.
- **Limits:** narrower type than `readData`/`writeData` — takes
  `OperationContext*` specifically, not the general `Context*`. A Backend
  implementer can't reuse this for arraystruct-context deletes without a
  cast/redesign.

### `beginArraystructAction`
```c++
virtual void beginArraystructAction(ArraystructContext *ctx, int *size) = 0;
```
- **Contract:** mirrors `al_begin_arraystruct_action`; `size` in/out.
- **Dependencies/Limits:** none beyond the general AOS contract.

---

## Cluster D — Introspection support

### `get_occurrences`
```c++
virtual void get_occurrences(Context* ctx, const char* ids_name, int** occurrences_list, int* size) = 0;
```
- **Contract:** mirrors `al_get_occurrences`.

### `list_filled_paths`
```c++
virtual void list_filled_paths(Context* ctx, const char* dataobjectname, char*** path_list, int* size) = 0;
```
- **Limits — the concrete per-backend matrix** (verified against every
  backend's actual `.cpp`, not just the header's doc comment):

  | Backend | Implementation |
  |---|---|
  | `HDF5Backend` | Real — `H5Literate`-based (`hdf5_reader.cpp:1598`) |
  | `UDABackend` | Delegates to `local_backend_->list_filled_paths(...)` if `access_local_`, else queries the remote server |
  | `MDSplusBackend` | `throw ALBackendException("list_filled_paths is not implemented in the MDSplus Backend", LOG);` |
  | `AsciiBackend` | `throw ALBackendException("list_filled_paths is not implemented in the ASCII Backend", LOG);` |
  | `MemoryBackend` | `throw ALBackendException("list_filled_paths() is not implemented in the MemoryBackend", LOG);` |
  | `FlexbuffersBackend` | `throw ALBackendException("list_filled_paths is not implemented in the Serialize Backend", LOG);` |
  | `NoBackend` | throws if `verbose`, else `*size = 0` |

  Since this is pure virtual, every backend *must* provide a body — four of
  them satisfy the C++ contract by unconditionally throwing. A rewrite
  should decide deliberately whether to keep this "throw for unsupported
  backends" pattern or make it a queryable capability flag like Cluster E's
  `supports*` methods, so callers can check before calling instead of
  catching an exception.

---

## Cluster E — Optional capability negotiation

```c++
virtual bool supportsTimeDataInterpolation() = 0;
virtual void initDataInterpolationComponent() = 0;
virtual bool supportsTimeRangeOperation() = 0;
```
- **Contract:** a backend declares, via the two `supports*` flags, whether it
  natively handles time-slice interpolation and time-range queries. If
  either is `true`, `initBackend`'s factory (`al_backend.cpp:88`) *automatically*
  calls `initDataInterpolationComponent()` right after construction — the
  Backend implementer never calls this themselves; it's framework-driven.
- **Verified per-backend matrix:**

  | Backend | `supportsTimeDataInterpolation` | `supportsTimeRangeOperation` |
  |---|---|---|
  | HDF5 | `true` | `true` |
  | MDSplus | `true` | **`false`** |
  | UDA (local access) | delegates to `local_backend_` | delegates to `supportsTimeDataInterpolation()` (same value) |
  | UDA (remote access) | `true` iff remote plugin version `> 1.4.0` (`src/uda/uda_backend.cpp:1189-1223`) | same (delegates) |
  | Memory | `false` | `false` |
  | ASCII | `false` | `false` |
  | Flexbuffers | `false` | `false` |

- **Limits:**
  - The two flags are **independent, not implied by each other** — MDSplus
    is the proof: it supports interpolation but explicitly not time range. A
    Backend implementer (or a rewrite) must not assume one implies the
    other.
  - **The "belt-and-suspenders" reading of `initDataInterpolationComponent()`
    is wrong** — this was taken from the header doc comment ("throws a
    backend exception if the backend does not perform time data
    interpolation") without checking the bodies, exactly the trap this
    document's Method section warns about. In every backend the factory
    actually calls it for (HDF5, MDSplus, UDA — the ones where a `supports*`
    flag can be `true`), the body is an **empty no-op**
    (`src/hdf5/hdf5_backend.h:140`, `src/mdsplus/mdsplus_backend.h:259`,
    `src/uda/uda_backend.h:192`). The throwing bodies exist only in ASCII,
    Memory, and Flexbuffers (`src/ascii_backend.h:100`,
    `src/memory_backend.h:682`, `src/flexbuffers_backend.h:57`) — backends
    the factory's `if` never calls this method for at all, since both their
    `supports*` flags are `false`. So the factory-level gate and the
    per-backend guard are **mutually exclusive halves of one mechanism**,
    not two redundant layers: the factory only ever reaches a no-op, and the
    throwing bodies are dead code from the factory's perspective (they'd
    only fire if something called `initDataInterpolationComponent()`
    directly, bypassing the factory).

---

## Open items — Backend implementer audience

- `al_context.h` (`Context`/`OperationContext`/`DataEntryContext`/
  `ArraystructContext`) — the classes every `Backend` method above actually
  receives and dispatches on (e.g. `ctx->getType()`, `getDatapath()`,
  `getAccessmode()`) — is not yet inventoried as its own capability surface
  (see "Scope" note at the top of this document).
- `al_exception.h` (`ALException`, `ALBackendException`, etc., and
  `ALException::registerStatus` — how a thrown exception becomes the
  User-facing `al_status_t`) is referenced throughout Part 2 as "the
  error-signaling contract" but its own API is not inventoried.

---

# Part 3 — Plugin author audience

Someone implementing the plugin C++ interface hierarchy to hook into the
read/write pipeline for cross-cutting concerns (provenance, readback,
on-the-fly transforms), independent of whichever `Backend` is active.
Compiled as a separate `.so`, discovered and instantiated at runtime by the
User via `al_register_plugin`/`al_bind_plugin` (Part 1, Cluster 3).

**Global prerequisite, before any cluster below applies:** the `.so` must
export two plain C factory functions matching the typedefs in
`access_layer_base_plugin.h`:
```c++
typedef access_layer_base_plugin *create_t();
typedef void destroy_t(access_layer_base_plugin *);
```
`al_register_plugin` resolves these via `dlsym`; if either is missing, it
throws before any of the plugin's own methods are ever called. This sits
above all 5 clusters below — not tied to one of them — the same way the
`.so`-naming/`$IMAS_AL_PLUGINS` convention is a User-side dependency, not a
capability.

**Class hierarchy** (so the cluster boundaries below make sense):
```
provenance_plugin_feature                     (Cluster 1)
  └─ access_layer_base_plugin                  (Cluster 2, + Cluster 1)
       └─ access_layer_plugin                  (Cluster 3, + readback_plugin_feature → Cluster 4)
            └─ extended_access_layer_plugin    (Cluster 3, extra method)
```

---

## Cluster 1 — Provenance metadata

```c++
// provenance_plugin_feature.h — all pure virtual
virtual std::string getName() = 0;
virtual std::string getDescription() = 0;
virtual std::string getCommit() = 0;
virtual std::string getVersion() = 0;
virtual std::string getRepository() = 0;
virtual std::string getParameters() = 0;
```
- **Contract:** self-identification — lets the framework record which
  plugin (and which version/commit) touched the data. Consumed by the
  User-facing `al_write_plugins_metadata` and internally by
  `AccessLayerPluginManager::write_plugins_metadata`/
  `write_plugins_infrastructure_infos` (`access_layer_plugin_manager.h`).
- **Dependencies:** none to implement these — pure metadata.
- **Limits:** this is the base of the whole hierarchy
  (`access_layer_base_plugin : public provenance_plugin_feature`), so **every
  plugin must implement all 6 of these, even a minimal one that only does
  parameter configuration and nothing else.** Not optional for "thin" plugins.

---

## Cluster 2 — Parameter configuration

```c++
// access_layer_base_plugin.h
virtual void setParameter(const char *parameter_name, int datatype, int dim, int *size, void *data) = 0;  // pure virtual
void setParameters(const std::string &parameters);                                                          // concrete, inherited, NOT overridable-required
```
- **Contract:** `setParameter` (singular) is what a plugin author implements
  to receive one typed parameter, reached via the User's
  `al_setvalue_*_parameter_plugin` calls. `setParameters` (plural) is a
  provided, already-implemented convenience that just stores a raw
  parameter string — **not** something the plugin author needs to (or
  should) override.
- **Limits:** the near-identical names (`setParameter` vs. `setParameters`)
  are a real naming-confusion risk — worth renaming distinctly in a rewrite
  (e.g. `setTypedParameter` vs. `setParameterString`). Whether `dim`/`size`
  here is bound by `MAXDIM` like ordinary data isn't stated in this header —
  still an open item (see Part 1).

---

## Cluster 3 — Action lifecycle & data interception

```c++
// access_layer_plugin.h — all pure virtual
virtual void begin_global_action(int pulseCtx, const char* dataobjectname, const char* datapath, int mode, int opCtx) = 0;
virtual void begin_slice_action(int pulseCtx, const char* dataobjectname, int mode, double time, int interp, int opCtx) = 0;
virtual void begin_arraystruct_action(int ctxID, int *actxID, const char* fieldPath, const char* timeBasePath, int *arraySize) = 0;
virtual void end_action(int ctx) = 0;
virtual int  read_data(int ctx, const char* fieldPath, const char* timeBasePath, void **data, int datatype, int dim, int *size) = 0;
virtual void write_data(int ctxID, const char *field, const char *timebase, void *data, int datatype, int dim, int *size) = 0;
virtual plugin::OPERATION node_operation(const std::string &path) = 0;  // GET_ONLY | PUT_ONLY | PUT_AND_GET

// extended_access_layer_plugin.h — adds one more, pure virtual
virtual void begin_timerange_action(int pulseCtx, const char* dataobjectname, int mode, double tmin, double tmax, std::vector<double> dtime, int interp, int opCtx) = 0;
```
- **Contract:** these are the actual hooks the framework calls when a User
  triggers a get/put and this plugin is bound to the relevant path.
  `node_operation(path)` is checked **first**, per path — only plugins
  returning `GET_ONLY`/`PUT_AND_GET` get `read_data` called for that path;
  only `PUT_ONLY`/`PUT_AND_GET` get `write_data` called.
- **Dependencies:**
  - Only invoked at all if the User has registered *and bound* this plugin
    to at least one path (Part 1, Cluster 3).
  - `begin_timerange_action` is only reachable if the plugin implements
    `extended_access_layer_plugin`, not just the base `access_layer_plugin`.
    Verified in `LLplugin::beginTimeRangeActionPlugin`
    (`al_lowlevel.cpp:471`): it does a `dynamic_cast` to the extended type;
    **if the cast fails, it does not throw** — it just `printf`s a warning
    and silently skips calling the method. A base-only plugin bound during a
    time-range operation degrades silently, not loudly.
- **Limits:**
  - `read_data` returns `int` following the **same 0=not-found/1=success
    convention as `Backend::readData`** (Part 2, Cluster C) — a genuinely
    useful consistency for reimplementation. `write_data` returns `void` —
    asymmetric with `read_data`.
  - The standalone C reentry function for this specific method,
    `al_plugin_begin_timerange_action`, has a real, verified bug (see
    Cluster 5) — a plugin author implementing `begin_timerange_action` who
    needs to re-enter the low-level layer from inside it currently has no
    working path to do so.

---

## Cluster 4 — Readback metadata

```c++
// readback_plugin_feature.h — all pure virtual
virtual std::string getReadbackName(const std::string &path, int *application_index) = 0;
virtual std::string getReadbackDescription(const std::string &path) = 0;
virtual std::string getReadbackCommit(const std::string &path) = 0;
virtual std::string getReadbackVersion(const std::string &path) = 0;
virtual std::string getReadbackRepository(const std::string &path) = 0;
virtual std::string getReadbackParameters(const std::string &path) = 0;
```
- **Contract:** per-path declaration of which *other* plugin should be used
  to read back (e.g. decompress) data this plugin wrote — the
  `camera_ir_write` → `camera_ir` pairing from the framework docs.
  `getReadbackName`'s `*application_index` out-param supports **chaining**:
  per the framework docs, index `1` means "apply after the plugin at index
  `0`" when multiple readback plugins target the same path.
- **Dependencies:** managed from the User side via
  `al_bind_readback_plugins`/`al_unbind_readback_plugins` (Part 1, Cluster 3).
- **Limits:** same "not optional for thin plugins" issue as Cluster 1 —
  `access_layer_plugin : public access_layer_base_plugin, public
  readback_plugin_feature` means **every plugin implementing the action
  lifecycle (Cluster 3) must also implement all 6 of these**, even a plugin
  with no readback pairing to declare.

---

## Cluster 5 — Low-level reentry

```c++
// al_lowlevel.h — plain C, extern "C"
al_status_t al_plugin_begin_global_action(int ctx, const char *dataobjectname, const char* datapath, int rwmode, int *opctx);
al_status_t al_plugin_begin_slice_action(int ctx, const char *dataobjectname, int rwmode, double time, int interpmode, int *opctx);
al_status_t al_plugin_begin_arraystruct_action(int ctx, const char *path, const char *timebase, int *size, int *aosctx);
al_status_t al_plugin_begin_timerange_action(int ctx, const char *dataobjectname, int rwmode, double tmin, double tmax, const double* dtime, const double* dtime_shape, int interpmode, int *opctx);  // BROKEN, see Limits
al_status_t al_plugin_end_action(int ctx);
al_status_t al_plugin_read_data(int ctx, const char *fieldpath, const char *timebasepath, void **data, int datatype, int dim, int *size);
al_status_t al_plugin_write_data(int ctx, const char *fieldpath, const char *timebasepath, void *data, int datatype, int dim, int *size);
```
- **Contract:** lets a plugin's *own* compiled code (inside its own
  `read_data()`/`write_data()`/`begin_*_action()` from Cluster 3) call back
  into the low-level layer to reach the real backend data — **without**
  re-triggering plugin dispatch on itself (which would recurse forever).
  Confirmed genuinely used this way today: `access_layer_plugin_manager.cpp`
  calls `al_plugin_read_data`/`al_plugin_write_data` for its own provenance
  metadata handling.
- **Dependencies:** only meaningful called from *within* a plugin's own
  Cluster 3 method implementations — calling them from elsewhere defeats
  their purpose (recursion avoidance) and isn't a supported use case.
  Requires plain C (`extern "C"`) linkage since plugins are separately
  compiled `.so` files, possibly with a different toolchain than the core.
- **Limits:** **`al_plugin_begin_timerange_action` is broken as declared** —
  confirmed by tracing the compiled binary (`nm -gU` shows a C++-mangled
  symbol, not the plain C one the header promises) back to a
  declaration/definition type mismatch (`double* dtime_shape` in the header
  vs. `int* dtime_shape` in the implementation). This was introduced by
  commit `0d44bb6` ("converting dtime to vector for supporting resampling
  with non uniform time increment"), which changed the header parameter to
  `double* dtime_shape` while changing the implementation's parameter to
  `int* dtime_shape` in the same change. Commit `1917f25c` (a later,
  unrelated change adding `supportsTimeRangeOperation()`) only added `const`
  qualifiers to the already-mismatched signatures — it did not introduce the
  bug. Currently latent (nothing in this codebase calls it externally yet),
  but any plugin author who writes a timerange-aware
  `extended_access_layer_plugin` and needs reentrant access from inside it
  will hit an undefined-symbol link error. Filed upstream — see the issue
  draft in this session's history. The other six reentry functions (global,
  slice, arraystruct, end, read, write) are confirmed present as plain C
  symbols in the compiled library.

---

## Open items — Plugin author audience

- Whether `Cluster 2`'s `dim`/`size` in `setParameter` is bound by `MAXDIM`
  — still unresolved (cross-referenced from Part 1's open items too).
- `access_layer_plugin_manager.h` is public and exported (`write_plugins_metadata`,
  `bind_readback_plugins`/`unbind_readback_plugins`, `*_handler` methods) but
  only referenced in passing from Cluster 1 — not separately inventoried
  (see "Scope" note at the top of this document).

---

## Summary — all three audiences inventoried, independently reviewed

This completes the first full pass: **User** (Part 1, 4 clusters), **Backend
implementer** (Part 2, 5 clusters), **Plugin author** (Part 3, 5 clusters +
1 global prerequisite). Every capability is grounded in verified code
(actual implementation bodies, the compiled symbol table, and git history
where relevant), not header doc comments taken at face value — several
header comments in this codebase turned out to be stale, imprecise, or
contradicted by the actual per-backend behavior.

An independent review pass (2026-07-03) re-verified every function-name/
file-line/return-convention claim and checked function-level coverage of the
three declared scopes against the canonical `PUBLIC_HEADER_FILES` list in
`CMakeLists.txt`. It found and this document has since corrected 12
correctness issues — including a misattributed commit for the
`al_plugin_begin_timerange_action` bug (now correctly traced to `0d44bb6`),
an incorrect "belt-and-suspenders" claim about `initDataInterpolationComponent()`
that had itself been taken from a doc comment without checking the bodies,
an under-documented `datapath`/UDA interaction, and a missed null-dereference
crash in `al_setvalue_*_parameter_plugin` on an unregistered plugin name.
It also confirmed that function-level coverage of the three declared
audiences is complete, while surfacing that the document's *scope* itself
was too narrow: five public headers (`al_context.h`, `al_exception.h`,
`uri_parser.h`, `data_interpolation.h`, `access_layer_plugin_manager.h`) and
the exported C++ classes inside `al_lowlevel.h` (`Lowlevel`, `LLplugin`,
`LLenv`) were neither inventoried nor explicitly declared out of scope. That
scope gap is now stated explicitly (see "Scope" at the top of this document)
rather than left as a silent omission. The open items lists across all three
parts are the known gaps for a follow-up pass, not silent omissions.
