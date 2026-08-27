# Recording-stub seam suites.

include_guard(GLOBAL)

# --- Runtime-binding test: proves the architecture (issue #3) ---------
#
# A recording stub stands in for IMAS-Core: it exports the complete
# runtime-bound utility, data, and plugin surface and records what it
# received, so assertions can be made directly on what crossed the
# boundary instead of inferring it from a data round trip.
#
# The stub is deliberately never linked into runtime_binding_test: doing so would
# give the linker two candidate definitions of al_context_info (the
# shim's and the stub's) to choose between — exactly the ambiguity
# runtime binding exists to avoid. runtime_binding_test only dlopen's the stub
# itself, purely to read back its recorded state.
add_executable(runtime_binding_test
    "${CMAKE_CURRENT_SOURCE_DIR}/tests/shim/runtime_binding_test.c")
target_link_libraries(runtime_binding_test PRIVATE imas_mvdd_loader ${CMAKE_DL_LIBS})
target_compile_definitions(runtime_binding_test PRIVATE
    "RECORDING_STUB_PATH=\"$<TARGET_FILE:recording_stub>\""
    "SHIM_LIBRARY_PATH=\"$<TARGET_FILE:imas_mvdd_loader>\""
    "SUPPORTED_CORE_VERSION=\"${IMAS_CORE_VERSION}\""
    "INCOMPATIBLE_CORE_VERSION=\"${IMAS_CORE_INCOMPATIBLE_VERSION}\"")
add_dependencies(runtime_binding_test imas_mvdd_capi recording_stub)
set_target_properties(runtime_binding_test PROPERTIES
    BUILD_RPATH "${IMAS_MVDD_STAGE_DIR}/lib")

add_stub_test(runtime-binding-success runtime_binding_test success)

add_test(NAME runtime-binding-version-drift-tolerated
    COMMAND runtime_binding_test version-drift)
set_tests_properties(runtime-binding-version-drift-tolerated PROPERTIES
    ENVIRONMENT
        "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>;RECORDING_STUB_VERSION=${IMAS_CORE_DRIFT_VERSION}"
    PASS_REGULAR_EXPRESSION
        "tolerating IMAS-Core version drift.*built against ${IMAS_CORE_VERSION}, found ${IMAS_CORE_DRIFT_VERSION}")

add_stub_test(runtime-binding-version-mismatch runtime_binding_test version-mismatch
    ENV "RECORDING_STUB_VERSION=${IMAS_CORE_INCOMPATIBLE_VERSION}")

add_stub_test(runtime-binding-null-version runtime_binding_test null-version
    ENV "RECORDING_STUB_NULL_VERSION=1")

add_test(NAME runtime-binding-missing-library COMMAND runtime_binding_test missing-library)
set_tests_properties(runtime-binding-missing-library PROPERTIES ENVIRONMENT
    "IMAS_CORE_LIBRARY=${CMAKE_CURRENT_BINARY_DIR}/does-not-exist.so")

add_stub_test(runtime-binding-verbatim-forwarding runtime_binding_test verbatim-forwarding)

add_stub_test(runtime-binding-plugin-forwarding runtime_binding_test plugin-forwarding)

add_test(NAME runtime-binding-plugin-timerange-omitted
    COMMAND runtime_binding_test plugin-timerange-omitted)

add_stub_test(runtime-binding-utility-forwarding runtime_binding_test utility-forwarding)

add_test(NAME runtime-binding-bare-soname COMMAND runtime_binding_test bare-soname)
if(APPLE)
    set(_runtime_binding_search_path_var DYLD_LIBRARY_PATH)
else()
    set(_runtime_binding_search_path_var LD_LIBRARY_PATH)
endif()
set_tests_properties(runtime-binding-bare-soname PROPERTIES ENVIRONMENT
    "${_runtime_binding_search_path_var}=$<TARGET_FILE_DIR:recording_stub>")

# --- HLI DD version latch test: the setter and its environment fallback
# (issue #45, ADR 0005) ---
#
# LATCH is a single process-wide OnceLock, so each scenario needs its own
# fresh process exactly like runtime_binding_test's scenarios do; ctest
# gives each add_test its own process already.
find_package(Threads REQUIRED)
add_executable(hli_dd_version_test
    "${CMAKE_CURRENT_SOURCE_DIR}/tests/shim/hli_dd_version_test.c")
target_link_libraries(hli_dd_version_test PRIVATE
    imas_mvdd_loader ${CMAKE_DL_LIBS} Threads::Threads)
target_compile_definitions(hli_dd_version_test PRIVATE
    "RECORDING_STUB_PATH=\"$<TARGET_FILE:recording_stub>\"")
add_dependencies(hli_dd_version_test imas_mvdd_capi recording_stub)
set_target_properties(hli_dd_version_test PROPERTIES
    BUILD_RPATH "${IMAS_MVDD_STAGE_DIR}/lib")

add_test(NAME hli-dd-version-setter-accepts-valid-version
    COMMAND hli_dd_version_test setter-accepts-valid-version)
add_test(NAME hli-dd-version-setter-accepts-identical-repeat
    COMMAND hli_dd_version_test setter-accepts-identical-repeat)
add_test(NAME hli-dd-version-setter-rejects-conflicting-repeat
    COMMAND hli_dd_version_test setter-rejects-conflicting-repeat)
add_test(NAME hli-dd-version-setter-rejects-invalid-version
    COMMAND hli_dd_version_test setter-rejects-invalid-version)
add_test(NAME hli-dd-version-setter-rejects-null-version
    COMMAND hli_dd_version_test setter-rejects-null-version)
add_test(NAME hli-dd-version-concurrent-identical-setters-all-succeed
    COMMAND hli_dd_version_test concurrent-identical-setters-all-succeed)

add_stub_test(hli-dd-version-setter-precedes-environment
    hli_dd_version_test setter-precedes-environment
    HLI_DD_VERSION not-a-version)

add_stub_test(hli-dd-version-valid-environment-latches-on-first-open
    hli_dd_version_test valid-environment-latches-on-first-open
    HLI_DD_VERSION 4.1.1)

add_stub_test(hli-dd-version-invalid-environment-fails-first-open
    hli_dd_version_test invalid-environment-fails-first-open
    HLI_DD_VERSION not-a-version)

add_test(NAME hli-dd-version-unset-first-open-then-setter-refused
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_MVDD_HLI_DD_VERSION --
        $<TARGET_FILE:hli_dd_version_test> unset-first-open-then-setter-refused)
set_tests_properties(hli-dd-version-unset-first-open-then-setter-refused PROPERTIES
    ENVIRONMENT "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")

# --- DD-version stamp discovery test: the al_begin_global_action seam
# and al_begin_dataentry_action's registration (issue #53, ADR 0002,
# ADR 0007, ADR 0009, ADR 0012) ---
#
# The HLI DD version latch and the context registry are both process-
# wide, so each scenario needs its own fresh process, same as
# hli_dd_version_test's scenarios.
add_executable(version_discovery_test
    "${CMAKE_CURRENT_SOURCE_DIR}/tests/shim/version_discovery_test.c")
target_link_libraries(version_discovery_test PRIVATE
    imas_mvdd_loader ${CMAKE_DL_LIBS})
target_compile_definitions(version_discovery_test PRIVATE
    "RECORDING_STUB_PATH=\"$<TARGET_FILE:recording_stub>\"")
add_dependencies(version_discovery_test imas_mvdd_capi recording_stub)
set_target_properties(version_discovery_test PROPERTIES
    BUILD_RPATH "${IMAS_MVDD_STAGE_DIR}/lib")

add_stub_test(version-discovery-dataentry-success-forwards-uri-and-mode version_discovery_test dataentry-success-forwards-uri-and-mode)

add_stub_test(version-discovery-dataentry-failure-forwards-status-unchanged
    version_discovery_test dataentry-failure-forwards-status-unchanged
    ENV "RECORDING_STUB_DATAENTRY_FAIL=1")

add_test(NAME version-discovery-hli-unset-global-action-is-plain-forward
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_MVDD_HLI_DD_VERSION --
        $<TARGET_FILE:version_discovery_test> hli-unset-global-action-is-plain-forward)
set_tests_properties(version-discovery-hli-unset-global-action-is-plain-forward PROPERTIES
    ENVIRONMENT "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")

add_stub_test(version-discovery-unstamped-occurrence-forwards-datapath-unchanged
    version_discovery_test unstamped-occurrence-forwards-datapath-unchanged
    HLI_DD_VERSION 4.1.1)

add_stub_test(version-discovery-matching-version-forwards-datapath-unchanged
    version_discovery_test matching-version-forwards-datapath-unchanged
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 4.1.1)

add_stub_test(version-discovery-mismatch-translates-datapath-on-second-open
    version_discovery_test mismatch-translates-datapath-on-second-open
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

# ADR 0020: a write-mode open reads the stamp through a shim-owned read-mode
# context of its own. The two scenarios that pin what the probe *asks for*
# refuse it (RECORDING_STUB_PLUGIN_GLOBAL_FAIL), because the stub's plugin
# recorder resets its integer fields on every call, so the probe's own rwmode
# is only readable while its open is the last plugin call made.
add_stub_test(version-discovery-write-mode-open-probes-through-the-plugin-family
    version_discovery_test write-mode-open-probes-through-the-plugin-family
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(version-discovery-write-mode-probe-asks-for-a-read-context
    version_discovery_test write-mode-probe-asks-for-a-read-context
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0
    ENV "RECORDING_STUB_PLUGIN_GLOBAL_FAIL=1")

add_stub_test(version-discovery-write-mode-slice-open-probes-with-a-global-action
    version_discovery_test write-mode-slice-open-probes-with-a-global-action
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0
    ENV "RECORDING_STUB_PLUGIN_GLOBAL_FAIL=1")

add_stub_test(version-discovery-replace-mode-open-probes-too
    version_discovery_test replace-mode-open-probes-too
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0
    ENV "RECORDING_STUB_PLUGIN_GLOBAL_FAIL=1")

add_stub_test(version-discovery-write-mode-timerange-open-probes-with-a-global-action
    version_discovery_test write-mode-timerange-open-probes-with-a-global-action
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0
    ENV "RECORDING_STUB_PLUGIN_GLOBAL_FAIL=1")

add_stub_test(version-discovery-read-mode-open-does-not-probe
    version_discovery_test read-mode-open-does-not-probe
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(version-discovery-unstamped-stamp-clears-an-earlier-mismatch
    version_discovery_test unstamped-stamp-clears-an-earlier-mismatch
    HLI_DD_VERSION 4.1.1)

add_stub_test(version-discovery-failed-stamp-read-clears-an-earlier-mismatch
    version_discovery_test failed-stamp-read-clears-an-earlier-mismatch
    HLI_DD_VERSION 4.1.1)

add_stub_test(version-discovery-malformed-stamp-refuses-and-ends-context
    version_discovery_test malformed-stamp-refuses-and-ends-context
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION not-a-version)

add_stub_test(version-discovery-reentrant-read-beneath-stamp-discovery-forwards-unchanged
    version_discovery_test reentrant-read-beneath-stamp-discovery-forwards-unchanged
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

# --- al_begin_slice_action / al_begin_timerange_action apply the same
# rule as global action (issue #55) ---

add_test(NAME version-discovery-slice-action-hli-unset-is-plain-forward
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_MVDD_HLI_DD_VERSION --
        $<TARGET_FILE:version_discovery_test> slice-action-hli-unset-is-plain-forward)
set_tests_properties(version-discovery-slice-action-hli-unset-is-plain-forward PROPERTIES
    ENVIRONMENT "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")

add_stub_test(version-discovery-slice-action-unstamped-forwards-ids-name-unchanged
    version_discovery_test slice-action-unstamped-forwards-ids-name-unchanged
    HLI_DD_VERSION 4.1.1)

add_stub_test(version-discovery-slice-action-matching-version-forwards-ids-name-unchanged
    version_discovery_test slice-action-matching-version-forwards-ids-name-unchanged
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 4.1.1)

add_stub_test(version-discovery-slice-action-mismatch-registers-occurrence-for-global-action
    version_discovery_test slice-action-mismatch-registers-occurrence-for-global-action
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(version-discovery-slice-action-malformed-stamp-refuses-and-ends-context
    version_discovery_test slice-action-malformed-stamp-refuses-and-ends-context
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION not-a-version)

add_stub_test(version-discovery-slice-action-failure-forwards-status-unchanged
    version_discovery_test slice-action-failure-forwards-status-unchanged
    HLI_DD_VERSION 4.1.1
    ENV "RECORDING_STUB_SLICE_FAIL=1")

add_test(NAME version-discovery-timerange-action-hli-unset-is-plain-forward
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_MVDD_HLI_DD_VERSION --
        $<TARGET_FILE:version_discovery_test> timerange-action-hli-unset-is-plain-forward)
set_tests_properties(version-discovery-timerange-action-hli-unset-is-plain-forward PROPERTIES
    ENVIRONMENT "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")

add_stub_test(version-discovery-timerange-action-unstamped-forwards-ids-name-unchanged
    version_discovery_test timerange-action-unstamped-forwards-ids-name-unchanged
    HLI_DD_VERSION 4.1.1)

add_stub_test(version-discovery-timerange-action-matching-version-forwards-ids-name-unchanged
    version_discovery_test timerange-action-matching-version-forwards-ids-name-unchanged
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 4.1.1)

add_stub_test(version-discovery-timerange-action-mismatch-registers-occurrence-for-global-action
    version_discovery_test timerange-action-mismatch-registers-occurrence-for-global-action
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(version-discovery-timerange-action-malformed-stamp-refuses-and-ends-context
    version_discovery_test timerange-action-malformed-stamp-refuses-and-ends-context
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION not-a-version)

add_stub_test(version-discovery-timerange-action-failure-forwards-status-unchanged
    version_discovery_test timerange-action-failure-forwards-status-unchanged
    HLI_DD_VERSION 4.1.1
    ENV "RECORDING_STUB_TIMERANGE_FAIL=1")

# --- Issue #56: ordinary read-path outcomes through the public C ABI ---
add_executable(read_path_test
    "${CMAKE_CURRENT_SOURCE_DIR}/tests/shim/read_path_test.c")
target_link_libraries(read_path_test PRIVATE imas_mvdd_loader ${CMAKE_DL_LIBS})
target_compile_definitions(read_path_test PRIVATE
    "RECORDING_STUB_PATH=\"$<TARGET_FILE:recording_stub>\"")
add_dependencies(read_path_test imas_mvdd_capi recording_stub)
set_target_properties(read_path_test PROPERTIES
    BUILD_RPATH "${IMAS_MVDD_STAGE_DIR}/lib")

add_stub_test(read-path-translates-field-and-timebase-independently
    read_path_test translates-field-and-timebase-independently
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(read-path-forward-direction-translates-and-reports-no-source
    read_path_test forward-direction-translates-and-reports-no-source
    HLI_DD_VERSION 3.39.0
    STAMP_VERSION 4.1.1)

add_stub_test(read-path-identity-rule-returns-data read_path_test identity-rule-returns-data
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(read-path-merged-read-falls-through-to-next-candidate
    read_path_test merged-read-falls-through-to-next-candidate
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0
    ENV "RECORDING_STUB_READ_NOT_FOUND_FIELD=time_slice/ggd/b_field_phi")
add_stub_test(read-path-merged-read-stops-at-first-candidate-with-data
    read_path_test merged-read-stops-at-first-candidate-with-data
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)
add_stub_test(read-path-merged-read-returns-not-found-when-all-candidates-are-absent
    read_path_test merged-read-returns-not-found-when-all-candidates-are-absent
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0
    ENV "RECORDING_STUB_READ_NOT_FOUND=1")
add_stub_test(read-path-split-plan-reads-and-flips-its-first-stored-destination
    read_path_test split-plan-reads-and-flips-its-first-stored-destination
    HLI_DD_VERSION 3.39.0
    STAMP_VERSION 4.1.1
    ENV "RECORDING_STUB_READ_DOUBLE=1")
add_stub_test(read-path-reverse-split-read-flips-its-single-stored-source
    read_path_test reverse-split-read-flips-its-single-stored-source
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0
    ENV "RECORDING_STUB_READ_DOUBLE=1")

add_stub_test(read-path-no-source-returns-null-without-core-call
    read_path_test no-source-returns-null-without-core-call
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

# All conversion-refusal scenarios need the same known mismatched
# occurrence. Keep that shared seam setup in one place.
function(add_read_path_refusal_test name scenario)
    add_stub_test("${name}" read_path_test "${scenario}"
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)
endfunction()

add_read_path_refusal_test(read-path-rank-changing-retype-refuses-without-core-call
    rank-changing-retype-refuses-without-core-call)
add_read_path_refusal_test(read-path-unit-redefinition-refuses-without-core-call
    unit-redefinition-refuses-without-core-call)
add_read_path_refusal_test(read-path-unsupported-sign-flip-types-refuse-without-core-call
    unsupported-sign-flip-types-refuse-without-core-call)
add_read_path_refusal_test(read-path-sign-flip-rank-exceeding-maxdim-refuses-without-core-call
    sign-flip-rank-exceeding-maxdim-refuses-without-core-call)

add_stub_test(read-path-sign-flip-array-negates-values-and-preserves-empty-double
    read_path_test sign-flip-array-negates-values-and-preserves-empty-double
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0
    ENV "RECORDING_STUB_READ_DOUBLE_VALUES=1.5,-9e40,3.2,-4.0")

# ADR 0014: a read re-entering the shim beneath an in-flight read is
# forwarded untouched. The stub's reentrant-read knob reproduces on any
# platform what real IMAS-Core only does on ELF.
add_stub_test(read-path-reentrant-read-is-forwarded-unchanged
    read_path_test reentrant-read-is-forwarded-unchanged
    HLI_DD_VERSION 3.39.0
    STAMP_VERSION 4.1.1)

add_stub_test(read-path-reentrant-read-does-not-reapply-a-sign-flip
    read_path_test reentrant-read-does-not-reapply-a-sign-flip
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0
    ENV "RECORDING_STUB_READ_DOUBLE_VALUES=1.5,-9e40,3.2,-4.0")

add_stub_test(read-path-plugin-reentrant-read-is-forwarded-across-the-ordinary-family
    read_path_test plugin-reentrant-read-is-forwarded-across-the-ordinary-family
    HLI_DD_VERSION 3.39.0
    STAMP_VERSION 4.1.1)

add_stub_test(read-path-sign-flip-invalid-shape-refuses-without-modifying-buffer
    read_path_test sign-flip-invalid-shape-refuses-without-modifying-buffer
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0
    ENV "RECORDING_STUB_READ_DOUBLE=1"
    "RECORDING_STUB_READ_SIZE_CSV=2147483647,2147483647,2147483647")

add_stub_test(read-path-sign-flip-shape-override-respects-read-rank
    read_path_test sign-flip-shape-override-respects-read-rank
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0
    ENV "RECORDING_STUB_READ_DOUBLE=1"
    "RECORDING_STUB_READ_SIZE_CSV=1,42")

add_stub_test(read-path-sign-flip-not-found-skips-value-transformation
    read_path_test sign-flip-not-found-skips-value-transformation
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0
    ENV "RECORDING_STUB_READ_NOT_FOUND=1")

add_stub_test(read-path-resolves-relative-field-and-absolute-timebase
    read_path_test resolves-relative-field-and-absolute-timebase
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(read-path-matching-context-bypasses-conversion
    read_path_test matching-context-bypasses-conversion
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 4.1.1)

add_stub_test(read-path-unknown-context-bypasses-conversion
    read_path_test unknown-context-bypasses-conversion
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(read-path-unstamped-context-bypasses-conversion
    read_path_test unstamped-context-bypasses-conversion
    HLI_DD_VERSION 4.1.1)

add_test(NAME read-path-conversion-disabled-bypasses-conversion
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_MVDD_HLI_DD_VERSION --
        $<TARGET_FILE:read_path_test> conversion-disabled-bypasses-conversion)
set_tests_properties(read-path-conversion-disabled-bypasses-conversion PROPERTIES
    ENVIRONMENT "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")

add_stub_test(read-path-core-failure-propagates-unchanged
    read_path_test core-failure-propagates-unchanged
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0
    ENV "RECORDING_STUB_READ_FAIL=1")

# --- Issues #65 and #124: root-context loss log and query exports ---
# Both scenarios below need the HLI reading in its own, 3.39.0 spelling
# (Direction::Forward) to hit a rule this artifact declares lossy in that
# direction: fold-ggd-bfield (merged) and move-gap (moved).
add_stub_test(read-path-merged-read-retains-a-lossy-verdict-in-the-loss-log
    read_path_test merged-read-retains-a-lossy-verdict-in-the-loss-log
    HLI_DD_VERSION 3.39.0
    STAMP_VERSION 4.1.1)

add_stub_test(read-path-moved-read-retains-a-lossy-verdict-in-the-loss-log
    read_path_test moved-read-retains-a-lossy-verdict-in-the-loss-log
    HLI_DD_VERSION 3.39.0
    STAMP_VERSION 4.1.1)

add_stub_test(read-path-ending-context-destroys-its-loss-log
    read_path_test ending-context-destroys-its-loss-log
    HLI_DD_VERSION 3.39.0
    STAMP_VERSION 4.1.1)

# The remaining query-export safety scenarios only need any live
# non-exact loss entry to query against, so they reuse the standard
# mismatched occurrence fixture like the refusal scenarios above.
function(add_loss_query_test name scenario)
    add_stub_test("${name}" read_path_test "${scenario}"
        HLI_DD_VERSION 3.39.0
        STAMP_VERSION 4.1.1)
endfunction()

add_loss_query_test(read-path-loss-count-null-output-is-refused
    loss-count-null-output-is-refused)
add_loss_query_test(read-path-loss-at-null-path-buffer-is-refused
    loss-at-null-path-buffer-is-refused)
add_loss_query_test(read-path-loss-at-null-verdict-is-refused
    loss-at-null-verdict-is-refused)
add_loss_query_test(read-path-loss-at-negative-index-is-refused
    loss-at-negative-index-is-refused)
add_loss_query_test(read-path-loss-at-out-of-range-index-is-refused
    loss-at-out-of-range-index-is-refused)
add_loss_query_test(read-path-loss-at-insufficient-buffer-is-refused
    loss-at-insufficient-buffer-is-refused)
add_loss_query_test(read-path-loss-operation-at-null-output-is-refused
    loss-operation-at-null-output-is-refused)
add_loss_query_test(read-path-loss-operation-at-negative-index-is-refused
    loss-operation-at-negative-index-is-refused)
add_loss_query_test(read-path-loss-operation-at-out-of-range-index-is-refused
    loss-operation-at-out-of-range-index-is-refused)
add_loss_query_test(read-path-loss-operation-at-untracked-context-is-refused-after-zero-count
    loss-operation-at-untracked-context-is-refused-after-zero-count)

# --- Issue #64: al_write_data / al_delete_data refusal through the public C ABI ---
add_executable(write_delete_conversion_test
    "${CMAKE_CURRENT_SOURCE_DIR}/tests/shim/write_delete_conversion_test.c")
target_link_libraries(write_delete_conversion_test PRIVATE imas_mvdd_loader ${CMAKE_DL_LIBS})
target_compile_definitions(write_delete_conversion_test PRIVATE
    "RECORDING_STUB_PATH=\"$<TARGET_FILE:recording_stub>\"")
add_dependencies(write_delete_conversion_test imas_mvdd_capi recording_stub)
set_target_properties(write_delete_conversion_test PROPERTIES
    BUILD_RPATH "${IMAS_MVDD_STAGE_DIR}/lib")

# All scenarios below need the same known mismatched equilibrium
# occurrence. Keep that shared seam setup in one place.
function(add_write_delete_mismatched_test name scenario)
    add_stub_test("${name}" write_delete_conversion_test "${scenario}"
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)
endfunction()

add_write_delete_mismatched_test(write-delete-write-renamed-field-lands-at-stored-spelling
    write-renamed-field-lands-at-stored-spelling)
add_write_delete_mismatched_test(write-delete-write-identity-and-moved-fields-land-at-stored-spelling
    write-identity-and-moved-fields-land-at-stored-spelling)
add_stub_test(write-delete-write-reverse-identity-renamed-and-moved-fields-land-at-stored-spelling
    write_delete_conversion_test write-reverse-identity-renamed-and-moved-fields-land-at-stored-spelling
    HLI_DD_VERSION 3.39.0
    STAMP_VERSION 4.1.1)
add_write_delete_mismatched_test(write-delete-delete-identity-renamed-and-moved-fields-land-at-stored-spelling
    delete-identity-renamed-and-moved-fields-land-at-stored-spelling)
add_stub_test(write-delete-delete-reverse-identity-renamed-and-moved-fields-land-at-stored-spelling
    write_delete_conversion_test delete-reverse-identity-renamed-and-moved-fields-land-at-stored-spelling
    HLI_DD_VERSION 3.39.0
    STAMP_VERSION 4.1.1)
add_write_delete_mismatched_test(write-delete-plugin-write-renamed-field-lands-at-stored-spelling
    plugin-write-renamed-field-lands-at-stored-spelling)
add_write_delete_mismatched_test(write-delete-write-nested-child-context-resolves-relative-and-absolute-fields
    write-nested-child-context-resolves-relative-and-absolute-fields)
add_stub_test(write-delete-write-candidate-lands-at-primary-and-retains-unwritten-candidates
    write_delete_conversion_test write-candidate-lands-at-primary-and-retains-unwritten-candidates
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0
    ENV "RECORDING_STUB_READ_LAST_WRITE=1")
add_write_delete_mismatched_test(write-delete-write-non-primary-source-refuses-by-precedence
    write-non-primary-source-refuses-by-precedence)
add_stub_test(write-delete-write-split-candidate-lands-at-primary
    write_delete_conversion_test write-split-candidate-lands-at-primary
    HLI_DD_VERSION 3.39.0
    STAMP_VERSION 4.1.1)
add_write_delete_mismatched_test(write-delete-child-write-candidate-retains-complete-path-at-root
    child-write-candidate-retains-complete-path-at-root)
add_write_delete_mismatched_test(write-delete-write-uses-the-primary-candidate-without-fanout
    write-uses-the-primary-candidate-without-fanout)
add_write_delete_mismatched_test(write-delete-write-cocos-sign-flip-uses-a-shim-owned-rank-seven-copy
    write-cocos-sign-flip-uses-a-shim-owned-rank-seven-copy)
add_write_delete_mismatched_test(write-delete-plugin-write-cocos-sign-flip-uses-a-shim-owned-copy
    plugin-write-cocos-sign-flip-uses-a-shim-owned-copy)
add_write_delete_mismatched_test(write-delete-write-cocos-sentinel-forwards-unchanged-without-loss
    write-cocos-sentinel-forwards-unchanged-without-loss)
add_write_delete_mismatched_test(write-delete-write-cocos-invalid-shape-or-type-refuses-before-core
    write-cocos-invalid-shape-or-type-refuses-before-core)
add_write_delete_mismatched_test(write-delete-write-refuses-dd-version-stamp-but-forwards-its-siblings
    write-refuses-dd-version-stamp-but-forwards-its-siblings)
add_write_delete_mismatched_test(write-delete-write-without-stored-slot-refuses-and-retains-a-write-loss
    write-without-stored-slot-refuses-and-retains-a-write-loss)
add_stub_test(write-delete-write-reverse-without-stored-slot-refuses-and-retains-a-write-loss
    write_delete_conversion_test write-reverse-without-stored-slot-refuses-and-retains-a-write-loss
    HLI_DD_VERSION 3.39.0
    STAMP_VERSION 4.1.1)
add_write_delete_mismatched_test(write-delete-write-retyped-path-refuses-and-retains-a-write-loss
    write-retyped-path-refuses-and-retains-a-write-loss)
add_write_delete_mismatched_test(write-delete-child-write-refusal-is-retained-on-its-root-with-a-complete-path
    child-write-refusal-is-retained-on-its-root-with-a-complete-path)
add_write_delete_mismatched_test(write-delete-delete-nested-child-context-translates-relative-path
    delete-nested-child-context-translates-relative-path)
add_write_delete_mismatched_test(write-delete-delete-refuses-stamp-subtrees-before-core-call
    delete-refuses-stamp-subtrees-before-core-call)
add_write_delete_mismatched_test(write-delete-delete-empty-path-forwards-as-explicit-migration-route
    delete-empty-path-forwards-as-explicit-migration-route)
add_write_delete_mismatched_test(write-delete-delete-refuses-no-source-unservable-and-structures
    delete-refuses-no-source-unservable-and-structures)
add_write_delete_mismatched_test(write-delete-delete-admits-trivial-structure-deletes
    delete-admits-trivial-structure-deletes)
add_stub_test(write-delete-delete-refuses-boundary-separatrix-reverse-direction
    write_delete_conversion_test delete-refuses-boundary-separatrix-reverse-direction
    HLI_DD_VERSION 3.39.0
    STAMP_VERSION 4.1.1)
add_write_delete_mismatched_test(write-delete-delete-fans-out-over-candidates-in-declared-order
    delete-fans-out-over-candidates-in-declared-order)
add_write_delete_mismatched_test(write-delete-delete-reports-a-failure-and-continues
    delete-reports-a-failure-and-continues)
add_stub_test(write-delete-delete-refuses-non-primary-source-before-core-call
    write_delete_conversion_test delete-refuses-non-primary-source-before-core-call
    HLI_DD_VERSION 3.39.0
    STAMP_VERSION 4.1.1)
add_stub_test(write-delete-write-refuses-non-primary-source-before-core-call
    write_delete_conversion_test write-refuses-non-primary-source-before-core-call
    HLI_DD_VERSION 3.39.0
    STAMP_VERSION 4.1.1)

add_stub_test(write-delete-write-unstamped-context-forwards-unchanged
    write_delete_conversion_test write-unstamped-context-forwards-unchanged
    HLI_DD_VERSION 4.1.1)

add_stub_test(write-delete-write-matching-context-forwards-unchanged
    write_delete_conversion_test write-matching-context-forwards-unchanged
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 4.1.1)

add_stub_test(write-delete-plugin-write-matching-context-forwards-unchanged
    write_delete_conversion_test plugin-write-matching-context-forwards-unchanged
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 4.1.1)

add_stub_test(write-delete-delete-matching-context-forwards-unchanged
    write_delete_conversion_test delete-matching-context-forwards-unchanged
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 4.1.1)

add_stub_test(write-delete-write-unknown-context-forwards-unchanged
    write_delete_conversion_test write-unknown-context-forwards-unchanged
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_test(NAME write-delete-write-conversion-disabled-forwards-unchanged
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_MVDD_HLI_DD_VERSION --
        $<TARGET_FILE:write_delete_conversion_test> write-conversion-disabled-forwards-unchanged)
set_tests_properties(write-delete-write-conversion-disabled-forwards-unchanged PROPERTIES
    ENVIRONMENT "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")

add_stub_test(write-delete-delete-unknown-context-forwards-unchanged
    write_delete_conversion_test delete-unknown-context-forwards-unchanged
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(write-delete-delete-unstamped-context-forwards-unchanged
    write_delete_conversion_test delete-unstamped-context-forwards-unchanged
    HLI_DD_VERSION 4.1.1)

add_test(NAME write-delete-delete-conversion-disabled-forwards-unchanged
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_MVDD_HLI_DD_VERSION --
        $<TARGET_FILE:write_delete_conversion_test> delete-conversion-disabled-forwards-unchanged)
set_tests_properties(write-delete-delete-conversion-disabled-forwards-unchanged PROPERTIES
    ENVIRONMENT "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")

# --- Issue #123: one reentry guard for every seam IMAS-Core calls beneath ---
add_executable(reentry_guard_test
    "${CMAKE_CURRENT_SOURCE_DIR}/tests/shim/reentry_guard_test.c")
target_link_libraries(reentry_guard_test PRIVATE imas_mvdd_loader ${CMAKE_DL_LIBS})
target_compile_definitions(reentry_guard_test PRIVATE
    "RECORDING_STUB_PATH=\"$<TARGET_FILE:recording_stub>\"")
add_dependencies(reentry_guard_test imas_mvdd_capi recording_stub)
set_target_properties(reentry_guard_test PROPERTIES
    BUILD_RPATH "${IMAS_MVDD_STAGE_DIR}/lib")

function(add_reentry_guard_test name scenario)
    add_stub_test("${name}" reentry_guard_test "${scenario}"
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0)
endfunction()

add_reentry_guard_test(reentry-guard-write-data-forwards-across-plugin-family
    write-data-reentry-forwards-across-the-plugin-family)
add_reentry_guard_test(reentry-guard-plugin-write-data-forwards-across-ordinary-family
    plugin-write-data-reentry-forwards-across-the-ordinary-family)
add_reentry_guard_test(reentry-guard-delete-data-forwards-unchanged
    delete-data-reentry-forwards-unchanged)
add_reentry_guard_test(reentry-guard-write-plugins-metadata-forwards-unchanged
    write-plugins-metadata-reentry-forwards-unchanged)
add_reentry_guard_test(reentry-guard-bind-readback-plugins-forwards-unchanged
    bind-readback-plugins-reentry-forwards-unchanged)
add_reentry_guard_test(reentry-guard-unbind-readback-plugins-forwards-unchanged
    unbind-readback-plugins-reentry-forwards-unchanged)

# --- Issue #61: arraystruct path conversion through the public C ABI ---
add_executable(arraystruct_path_test
    "${CMAKE_CURRENT_SOURCE_DIR}/tests/shim/arraystruct_path_test.c")
target_link_libraries(arraystruct_path_test PRIVATE imas_mvdd_loader ${CMAKE_DL_LIBS})
target_compile_definitions(arraystruct_path_test PRIVATE
    "RECORDING_STUB_PATH=\"$<TARGET_FILE:recording_stub>\"")
add_dependencies(arraystruct_path_test imas_mvdd_capi recording_stub)
set_target_properties(arraystruct_path_test PROPERTIES
    BUILD_RPATH "${IMAS_MVDD_STAGE_DIR}/lib")

add_stub_test(arraystruct-path-translates-renamed-container-and-timebase
    arraystruct_path_test translates-renamed-container-and-timebase
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(arraystruct-path-translates-absolute-path-and-relative-timebase
    arraystruct_path_test translates-absolute-path-and-relative-timebase
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(arraystruct-path-failed-open-propagates-without-child-record
    arraystruct_path_test failed-open-propagates-without-child-record
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0
    ENV "RECORDING_STUB_ARRAYSTRUCT_FAIL=1")

add_stub_test(arraystruct-path-no-source-refuses-before-core
    arraystruct_path_test no-source-refuses-before-core
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(arraystruct-path-matching-parent-forwards-unchanged
    arraystruct_path_test plain-parent-forwards-unchanged
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 4.1.1)

add_stub_test(arraystruct-path-unstamped-parent-forwards-unchanged
    arraystruct_path_test plain-parent-forwards-unchanged
    HLI_DD_VERSION 4.1.1)

add_stub_test(arraystruct-path-unknown-parent-forwards-unchanged
    arraystruct_path_test unknown-parent-forwards-unchanged
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_test(NAME arraystruct-path-conversion-disabled-parent-forwards-unchanged
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_MVDD_HLI_DD_VERSION --
        $<TARGET_FILE:arraystruct_path_test> plain-parent-forwards-unchanged)
set_tests_properties(arraystruct-path-conversion-disabled-parent-forwards-unchanged PROPERTIES
    ENVIRONMENT "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")

# --- Issue #62: al_read_data through a live arraystruct context -------
add_executable(nested_context_read_test
    "${CMAKE_CURRENT_SOURCE_DIR}/tests/shim/nested_context_read_test.c")
target_link_libraries(nested_context_read_test PRIVATE imas_mvdd_loader ${CMAKE_DL_LIBS})
target_compile_definitions(nested_context_read_test PRIVATE
    "RECORDING_STUB_PATH=\"$<TARGET_FILE:recording_stub>\"")
add_dependencies(nested_context_read_test imas_mvdd_capi recording_stub)
set_target_properties(nested_context_read_test PROPERTIES
    BUILD_RPATH "${IMAS_MVDD_STAGE_DIR}/lib")

add_stub_test(nested-context-read-relative-field-and-timebase-resolve-through-renamed-child
    nested_context_read_test relative-field-and-timebase-resolve-through-renamed-child
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(nested-context-read-absolute-field-outside-child-subtree-resolves-from-ids-root
    nested_context_read_test absolute-field-outside-child-subtree-resolves-from-ids-root
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(nested-context-read-no-source-returns-null-through-nested-child
    nested_context_read_test no-source-returns-null-through-nested-child
    HLI_DD_VERSION 3.39.0
    STAMP_VERSION 4.1.1)

add_stub_test(nested-context-read-refusal-stops-before-core-through-nested-child
    nested_context_read_test refusal-stops-before-core-through-nested-child
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(nested-context-read-sign-flip-applies-through-nested-child
    nested_context_read_test sign-flip-applies-through-nested-child
    HLI_DD_VERSION 3.39.0
    STAMP_VERSION 4.1.1
    ENV "RECORDING_STUB_READ_DOUBLE=1")

# --- Issue #66: nested non-exact reads attribute to their root's loss log ---
add_stub_test(nested-context-read-moved-read-through-nested-child-retains-lossy-verdict-on-root
    nested_context_read_test moved-read-through-nested-child-retains-lossy-verdict-on-root
    HLI_DD_VERSION 3.39.0
    STAMP_VERSION 4.1.1)

add_stub_test(nested-context-read-merged-read-through-nested-child-retains-potentially-lossy-verdict
    nested_context_read_test merged-read-through-nested-child-retains-potentially-lossy-verdict
    HLI_DD_VERSION 3.39.0
    STAMP_VERSION 4.1.1)

add_stub_test(nested-context-read-ending-root-before-child-destroys-the-shared-loss-log
    nested_context_read_test ending-root-before-child-destroys-the-shared-loss-log
    HLI_DD_VERSION 3.39.0
    STAMP_VERSION 4.1.1)
# --- Issue #63: context lifecycle (al_end_action,
# al_iterate_over_arraystruct, al_close_pulse) against the recording
# stub --------------------------------------------------------------
add_executable(context_lifecycle_test
    "${CMAKE_CURRENT_SOURCE_DIR}/tests/shim/context_lifecycle_test.c")
target_link_libraries(context_lifecycle_test PRIVATE imas_mvdd_loader ${CMAKE_DL_LIBS})
target_compile_definitions(context_lifecycle_test PRIVATE
    "RECORDING_STUB_PATH=\"$<TARGET_FILE:recording_stub>\"")
add_dependencies(context_lifecycle_test imas_mvdd_capi recording_stub)
set_target_properties(context_lifecycle_test PROPERTIES
    BUILD_RPATH "${IMAS_MVDD_STAGE_DIR}/lib")

add_stub_test(context-lifecycle-ending-child-removes-only-its-own-record
    context_lifecycle_test ending-child-removes-only-its-own-record
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(context-lifecycle-ending-root-removes-only-its-own-record
    context_lifecycle_test ending-root-removes-only-its-own-record
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(context-lifecycle-failed-end-action-leaves-the-record-intact
    context_lifecycle_test failed-end-action-leaves-the-record-intact
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(context-lifecycle-recycled-id-cannot-observe-the-released-record
    context_lifecycle_test recycled-id-cannot-observe-the-released-record
    HLI_DD_VERSION 4.1.1)

add_stub_test(context-lifecycle-iterate-over-arraystruct-forwards-unchanged-and-mutates-nothing
    context_lifecycle_test iterate-over-arraystruct-forwards-unchanged-and-mutates-nothing
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(context-lifecycle-close-pulse-forwards-unchanged-and-never-mutates-the-registry
    context_lifecycle_test close-pulse-forwards-unchanged-and-never-mutates-the-registry
    HLI_DD_VERSION 4.1.1)

add_stub_test(context-lifecycle-ending-dataentry-context-leaves-live-operation-and-child-records-intact
    context_lifecycle_test ending-dataentry-context-leaves-live-operation-and-child-records-intact
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

# --- Issue #67: give the al_plugin_* reentry twins the same context
# creation, path translation, and lifecycle behavior as their ordinary
# counterparts --------------------------------------------------------
add_executable(plugin_reentry_policy_test
    "${CMAKE_CURRENT_SOURCE_DIR}/tests/shim/plugin_reentry_policy_test.c")
target_link_libraries(plugin_reentry_policy_test PRIVATE imas_mvdd_loader ${CMAKE_DL_LIBS})
target_compile_definitions(plugin_reentry_policy_test PRIVATE
    "RECORDING_STUB_PATH=\"$<TARGET_FILE:recording_stub>\"")
add_dependencies(plugin_reentry_policy_test imas_mvdd_capi recording_stub)
set_target_properties(plugin_reentry_policy_test PROPERTIES
    BUILD_RPATH "${IMAS_MVDD_STAGE_DIR}/lib")

add_test(NAME plugin-reentry-policy-plugin-global-hli-unset-is-plain-forward
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_MVDD_HLI_DD_VERSION --
        $<TARGET_FILE:plugin_reentry_policy_test> plugin-global-hli-unset-is-plain-forward)
set_tests_properties(plugin-reentry-policy-plugin-global-hli-unset-is-plain-forward PROPERTIES
    ENVIRONMENT "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")

add_stub_test(plugin-reentry-policy-plugin-global-unstamped-forwards-datapath-unchanged
    plugin_reentry_policy_test plugin-global-unstamped-forwards-datapath-unchanged
    HLI_DD_VERSION 4.1.1)

add_stub_test(plugin-reentry-policy-plugin-global-matching-version-forwards-datapath-unchanged
    plugin_reentry_policy_test plugin-global-matching-version-forwards-datapath-unchanged
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 4.1.1)

add_stub_test(plugin-reentry-policy-plugin-global-mismatch-translates-datapath-on-second-open
    plugin_reentry_policy_test plugin-global-mismatch-translates-datapath-on-second-open
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(plugin-reentry-policy-plugin-global-malformed-stamp-refuses-and-ends-context
    plugin_reentry_policy_test plugin-global-malformed-stamp-refuses-and-ends-context
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION not-a-version)

add_stub_test(plugin-reentry-policy-plugin-global-failure-forwards-status-unchanged
    plugin_reentry_policy_test plugin-global-failure-forwards-status-unchanged
    HLI_DD_VERSION 4.1.1
    ENV "RECORDING_STUB_PLUGIN_GLOBAL_FAIL=1")

add_test(NAME plugin-reentry-policy-plugin-slice-hli-unset-is-plain-forward
    COMMAND "${CMAKE_COMMAND}" -E env --unset=IMAS_MVDD_HLI_DD_VERSION --
        $<TARGET_FILE:plugin_reentry_policy_test> plugin-slice-hli-unset-is-plain-forward)
set_tests_properties(plugin-reentry-policy-plugin-slice-hli-unset-is-plain-forward PROPERTIES
    ENVIRONMENT "IMAS_CORE_LIBRARY=$<TARGET_FILE:recording_stub>")

add_stub_test(plugin-reentry-policy-plugin-slice-mismatch-registers-occurrence-for-plugin-global-action
    plugin_reentry_policy_test plugin-slice-mismatch-registers-occurrence-for-plugin-global-action
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(plugin-reentry-policy-plugin-slice-malformed-stamp-refuses-and-ends-context
    plugin_reentry_policy_test plugin-slice-malformed-stamp-refuses-and-ends-context
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION not-a-version)

add_stub_test(plugin-reentry-policy-plugin-slice-failure-forwards-status-unchanged
    plugin_reentry_policy_test plugin-slice-failure-forwards-status-unchanged
    HLI_DD_VERSION 4.1.1
    ENV "RECORDING_STUB_PLUGIN_SLICE_FAIL=1")

add_stub_test(plugin-reentry-policy-plugin-arraystruct-translates-under-mismatch
    plugin_reentry_policy_test plugin-arraystruct-translates-under-mismatch
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(plugin-reentry-policy-plugin-arraystruct-failed-open-propagates-without-child-record
    plugin_reentry_policy_test plugin-arraystruct-failed-open-propagates-without-child-record
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0
    ENV "RECORDING_STUB_PLUGIN_ARRAYSTRUCT_FAIL=1")

add_stub_test(plugin-reentry-policy-plugin-arraystruct-no-source-refuses-before-core
    plugin_reentry_policy_test plugin-arraystruct-no-source-refuses-before-core
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(plugin-reentry-policy-plugin-arraystruct-unknown-parent-forwards-unchanged
    plugin_reentry_policy_test plugin-arraystruct-unknown-parent-forwards-unchanged
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(plugin-reentry-policy-plugin-end-action-removes-only-its-own-record
    plugin_reentry_policy_test plugin-end-action-removes-only-its-own-record
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(plugin-reentry-policy-plugin-end-action-failed-leaves-the-record-intact
    plugin_reentry_policy_test plugin-end-action-failed-leaves-the-record-intact
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

# --- Issue #68: al_plugin_read_data follows al_read_data's policy exactly ---
add_stub_test(plugin-reentry-policy-plugin-read-translates-field-under-mismatch
    plugin_reentry_policy_test plugin-read-translates-field-under-mismatch
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(plugin-reentry-policy-plugin-read-refusal-before-core
    plugin_reentry_policy_test plugin-read-refusal-before-core
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(plugin-reentry-policy-plugin-read-no-source-returns-null-without-core-call
    plugin_reentry_policy_test plugin-read-no-source-returns-null-without-core-call
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0)

add_stub_test(plugin-reentry-policy-plugin-read-merged-candidate-falls-through
    plugin_reentry_policy_test plugin-read-merged-candidate-falls-through
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0
    ENV "RECORDING_STUB_READ_NOT_FOUND_FIELD=time_slice/ggd/b_field_phi")

add_stub_test(plugin-reentry-policy-plugin-read-sign-flip-negates-values
    plugin_reentry_policy_test plugin-read-sign-flip-negates-values
    HLI_DD_VERSION 4.1.1
    STAMP_VERSION 3.39.0
    ENV "RECORDING_STUB_READ_DOUBLE=1")

add_stub_test(plugin-reentry-policy-plugin-read-through-child-context-retains-loss-on-root
    plugin_reentry_policy_test plugin-read-through-child-context-retains-loss-on-root
    HLI_DD_VERSION 3.39.0
    STAMP_VERSION 4.1.1)

# --- Issue #69: conversion stays inside its declared seam list ---------
#
# Every scenario runs with the same active conversion the seam tests above
# use — HLI 4.1.1 against a 3.39.0 stamp — and asserts the *absence* of
# translation outside that list. RECORDING_STUB_FILLED_PATHS_CSV makes
# IMAS-Core report a path the loaded artifact has a rename rule for, so
# "returned unchanged" is a claim about the shim rather than about a
# placeholder string no rule could have touched either way.
add_executable(scoped_passthrough_test
    "${CMAKE_CURRENT_SOURCE_DIR}/tests/shim/scoped_passthrough_test.c")
target_link_libraries(scoped_passthrough_test PRIVATE imas_mvdd_loader ${CMAKE_DL_LIBS})
target_compile_definitions(scoped_passthrough_test PRIVATE
    "RECORDING_STUB_PATH=\"$<TARGET_FILE:recording_stub>\""
    "EXPECTED_AL_VERSION=\"${IMAS_CORE_VERSION}\"")
add_dependencies(scoped_passthrough_test imas_mvdd_capi recording_stub)
set_target_properties(scoped_passthrough_test PROPERTIES
    BUILD_RPATH "${IMAS_MVDD_STAGE_DIR}/lib")

function(add_scoped_passthrough_test name scenario)
    add_stub_test("${name}" scoped_passthrough_test "${scenario}"
        HLI_DD_VERSION 4.1.1
        STAMP_VERSION 3.39.0
        ENV "RECORDING_STUB_FILLED_PATHS_CSV=time_slice/global_quantities/beta_normal,time_slice/global_quantities/ip")
endfunction()

add_scoped_passthrough_test(scoped-passthrough-get-occurrences-forwards-ids-name-unchanged
    get-occurrences-forwards-ids-name-unchanged)
add_scoped_passthrough_test(
    scoped-passthrough-list-filled-paths-forwards-name-and-returns-stored-paths-unchanged
    list-filled-paths-forwards-name-and-returns-stored-paths-unchanged)
add_scoped_passthrough_test(
    scoped-passthrough-bind-and-unbind-plugin-forward-field-path-unchanged
    bind-and-unbind-plugin-forward-field-path-unchanged)
add_scoped_passthrough_test(scoped-passthrough-remaining-non-seam-exports-forward-unchanged
    remaining-non-seam-exports-forward-unchanged)

# Issue #134: the same three path-bearing passthrough seams, asserted while a
# WRITE_OP-opened occurrence is demonstrably converting a write rather than a
# read. Same conversion setup and same seeded stored spelling — only the
# operation proving the conversion is active differs.
add_scoped_passthrough_test(scoped-passthrough-writing-get-occurrences-forwards-ids-name-unchanged
    writing-get-occurrences-forwards-ids-name-unchanged)
add_scoped_passthrough_test(
    scoped-passthrough-writing-list-filled-paths-forwards-name-and-returns-stored-paths-unchanged
    writing-list-filled-paths-forwards-name-and-returns-stored-paths-unchanged)
add_scoped_passthrough_test(
    scoped-passthrough-writing-bind-and-unbind-plugin-forward-field-path-unchanged
    writing-bind-and-unbind-plugin-forward-field-path-unchanged)
