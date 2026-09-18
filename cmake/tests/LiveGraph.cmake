# Private live-source instance. Credentials are inherited at test execution,
# never serialized into CMakeCache.txt or CTestTestfile.cmake.
set_target_properties(graph_runtime_map_test PROPERTIES IMAS_MVDD_CTEST_LABEL live-graph)
foreach(direction IN ITEMS forward reverse)
    if(direction STREQUAL "forward")
        set(hli 4.1.1)
        set(stored 3.39.0)
        set(suffix hli-new)
    else()
        set(hli 3.39.0)
        set(stored 4.1.1)
        set(suffix hli-old)
    endif()
    add_stub_test(live-graph-${direction}-identity graph_runtime_map_test identity-operations
        HLI_DD_VERSION ${hli} STAMP_VERSION ${stored})
    foreach(operation IN ITEMS read write delete)
        add_stub_test(live-graph-${direction}-rename-${operation} graph_runtime_map_test renamed-${operation}-${suffix}
            HLI_DD_VERSION ${hli} STAMP_VERSION ${stored})
    endforeach()
    add_stub_test(live-graph-${direction}-psi graph_runtime_map_test psi-read-flips-once-and-write-uses-its-inverse
        HLI_DD_VERSION ${hli} STAMP_VERSION ${stored}
        ENV "RECORDING_STUB_READ_DOUBLE_VALUES=1.5,-2.0,-9.0e40")
endforeach()
add_stub_test(live-graph-acquisition-failure graph_runtime_map_test acquisition-failure-cleans-up-open-context
    HLI_DD_VERSION 4.1.1 STAMP_VERSION 3.39.0)

target_compile_definitions(graph_runtime_map_test PRIVATE IMAS_MVDD_LIVE_GRAPH=1)
add_stub_test(live-graph-coexistence-anchor graph_runtime_map_test coexistence-arraystruct-keeps-the-opened-anchor
    HLI_DD_VERSION 4.1.1 STAMP_VERSION 3.42.0
    ENV "RECORDING_STUB_ARRAYSTRUCT_EMPTY_PATHS=time_slice/constraints/j_phi" "RECORDING_STUB_READ_SCALAR_VALUES=measured=42,/time_slice/constraints/j_phi/measured=42")
add_stub_test(live-graph-coexistence-empty graph_runtime_map_test coexistence-arraystruct-opens-when-every-candidate-is-empty
    HLI_DD_VERSION 4.1.1 STAMP_VERSION 3.42.0
    ENV "RECORDING_STUB_ARRAYSTRUCT_EMPTY_PATHS=time_slice/constraints/j_phi,time_slice/constraints/j_tor")
add_stub_test(live-graph-coexistence-primary graph_runtime_map_test coexistence-arraystruct-write-mode-uses-primary-without-probing
    HLI_DD_VERSION 4.1.1 STAMP_VERSION 3.42.0
    ENV "RECORDING_STUB_ARRAYSTRUCT_EMPTY_PATHS=time_slice/constraints/j_phi" "RECORDING_STUB_READ_SCALAR_VALUES=measured=42,/time_slice/constraints/j_phi/measured=42")
add_stub_test(live-graph-coexistence-plugin graph_runtime_map_test coexistence-arraystruct-plugin-twin-keeps-the-primary-anchor
    HLI_DD_VERSION 4.1.1 STAMP_VERSION 3.42.0
    ENV "RECORDING_STUB_READ_SCALAR_VALUES=measured=42")
foreach(scenario IN ITEMS reentrant-read-is-passthrough-under-graph-open passthrough-is-unchanged-under-graph-open
        failure-closes-every-opening-family cached-mismatch-translates-global-datapath write-mode-uses-its-read-op-stamp-probe)
    add_stub_test(live-graph-${scenario} graph_runtime_map_test ${scenario} HLI_DD_VERSION 4.1.1 STAMP_VERSION 3.39.0)
endforeach()
