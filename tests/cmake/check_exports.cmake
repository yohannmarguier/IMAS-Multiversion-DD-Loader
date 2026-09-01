# Script-mode CMake takes its policy version from this call alone. Without it
# CMake 3.x leaves CMP0057 unset, so `IN_LIST` below is not an operator and the
# test dies on "Unknown arguments specified"; CMake 4.x defaults the policy to
# NEW and hides the breakage, so the failure only ever shows up on CI.
cmake_minimum_required(VERSION 3.21)

# Compare the public C exports mechanically rather than maintaining a
# second handwritten manifest. Both nm variants used by CI are accepted:
# Mach-O prepends an underscore to C names, ELF does not.

foreach(required_variable CORE_LIBRARY SHIM_LIBRARY NM_EXECUTABLE)
    if(NOT DEFINED ${required_variable})
        message(FATAL_ERROR "${required_variable} must be supplied")
    endif()
endforeach()

function(all_exported_symbols library output_variable)
    execute_process(
        # Both nm variants used by CI spell “defined symbols only” as `-U`:
        # GNU nm otherwise prints imports such as GLIBC functions, while
        # Apple's spelling produces the same defined external-symbol set.
        COMMAND "${NM_EXECUTABLE}" -g -U "${library}"
        RESULT_VARIABLE nm_result
        OUTPUT_VARIABLE nm_output
        ERROR_VARIABLE nm_error)
    if(NOT nm_result EQUAL 0)
        message(FATAL_ERROR "nm failed for ${library}: ${nm_error}")
    endif()

    string(REPLACE "\n" ";" nm_lines "${nm_output}")
    set(symbols)
    foreach(line IN LISTS nm_lines)
        # The last whitespace-delimited field is nm's unmangled symbol name.
        # Extracting it first prevents a C++ mangled symbol that happens to
        # contain (for example) `al_begin_dataentry_action` from matching.
        separate_arguments(nm_fields UNIX_COMMAND "${line}")
        if(NOT nm_fields)
            continue()
        endif()
        list(POP_BACK nm_fields candidate)

        # Mach-O prepends one underscore to C symbols; ELF does not. Avoid an
        # anchored regular expression here because its repeated-match behavior
        # changed under CMake policy CMP0186.
        string(SUBSTRING "${candidate}" 0 1 first_character)
        if(first_character STREQUAL "_")
            string(SUBSTRING "${candidate}" 1 -1 symbol)
        else()
            set(symbol "${candidate}")
        endif()
        list(APPEND symbols "${symbol}")
    endforeach()
    set("${output_variable}" "${symbols}" PARENT_SCOPE)
endfunction()

# The mirrored IMAS-Core surface: the `al_` prefix plus four named
# exceptions. Deliberately excludes the `imas_mvdd_` prefix (see
# owned_abi_exports below) so a shim-owned export can never inflate this set
# and hide inside the mirrored-coverage comparison (ADR 0005).
function(mirrored_abi_exports library output_variable)
    all_exported_symbols("${library}" all_symbols)
    set(exports)
    foreach(symbol IN LISTS all_symbols)
        string(SUBSTRING "${symbol}" 0 3 symbol_prefix)
        if(symbol_prefix STREQUAL "al_" OR
                symbol STREQUAL "const2str" OR
                symbol STREQUAL "err2str" OR
                symbol STREQUAL "getALVersion" OR
                symbol STREQUAL "getDDVersion")
            list(APPEND exports "${symbol}")
        endif()
    endforeach()
    list(REMOVE_DUPLICATES exports)
    list(SORT exports)
    set("${output_variable}" "${exports}" PARENT_SCOPE)
endfunction()

# The shim-owned surface (ADR 0005 consequence): it is the complete set of
# shim exports outside the mirrored surface. Each must carry the `imas_mvdd_`
# prefix and appear on the declared owned-exports manifest, so no unrelated
# public export can escape this check.
function(owned_abi_exports library output_variable)
    all_exported_symbols("${library}" all_symbols)
    mirrored_abi_exports("${library}" mirrored_exports)
    set(exports "${all_symbols}")
    list(REMOVE_ITEM exports ${mirrored_exports})
    list(REMOVE_DUPLICATES exports)
    list(SORT exports)
    set("${output_variable}" "${exports}" PARENT_SCOPE)
endfunction()

mirrored_abi_exports("${CORE_LIBRARY}" core_exports)
mirrored_abi_exports("${SHIM_LIBRARY}" shim_mirrored_exports)
owned_abi_exports("${SHIM_LIBRARY}" shim_owned_exports)

# Keep the signature checker's X-macro manifest tied to the real exported
# surface. The strict entry shape makes a malformed or multiline symbol entry
# fail here instead of silently disappearing from signature coverage.
file(STRINGS "${CMAKE_CURRENT_LIST_DIR}/../abi/abi_symbols.def" manifest_lines
    REGEX "^IMAS_ABI_SYMBOL\\(")
set(manifest_exports)
foreach(line IN LISTS manifest_lines)
    if(NOT line MATCHES
            "^IMAS_ABI_SYMBOL\\(([A-Za-z0-9_]+),[ \t]*[A-Za-z0-9_]+\\)$")
        message(FATAL_ERROR "Malformed ABI manifest entry: ${line}")
    endif()
    list(APPEND manifest_exports "${CMAKE_MATCH_1}")
endforeach()
list(REMOVE_DUPLICATES manifest_exports)
list(SORT manifest_exports)

# Read the declared owned-exports manifest the same mechanical way.
file(STRINGS "${CMAKE_CURRENT_LIST_DIR}/../abi/owned_exports.def" owned_manifest_lines
    REGEX "^IMAS_MVDD_OWNED_EXPORT\\(")
set(owned_manifest_exports)
foreach(line IN LISTS owned_manifest_lines)
    if(NOT line MATCHES "^IMAS_MVDD_OWNED_EXPORT\\((imas_mvdd_[A-Za-z0-9_]+)\\)$")
        message(FATAL_ERROR "Malformed owned-export manifest entry: ${line}")
    endif()
    list(APPEND owned_manifest_exports "${CMAKE_MATCH_1}")
endforeach()
list(REMOVE_DUPLICATES owned_manifest_exports)
list(SORT owned_manifest_exports)
list(LENGTH owned_manifest_exports owned_export_count)
if(NOT owned_export_count EQUAL 4)
    message(FATAL_ERROR
        "The shim-owned export manifest must retain exactly four entries, got ${owned_export_count}")
endif()

# Assertion 1: every IMAS-Core symbol is present in the shim, and the shim
# introduces no extra symbol under IMAS-Core's own `al_`/exception surface.
if(NOT "${core_exports}" STREQUAL "${shim_mirrored_exports}")
    message(FATAL_ERROR
        "IMAS-Core public C exports differ from the shim's mirrored surface.\n"
        "Core: ${core_exports}\n"
        "Shim (mirrored): ${shim_mirrored_exports}")
endif()

if(NOT "${manifest_exports}" STREQUAL "${shim_mirrored_exports}")
    message(FATAL_ERROR
        "ABI signature manifest differs from the shim's mirrored exports.\n"
        "Manifest: ${manifest_exports}\n"
        "Shim (mirrored): ${shim_mirrored_exports}")
endif()

# Assertion 2: every extra shim symbol outside the mirrored surface appears
# on the explicit, declared owned-exports list — never a same-namespace
# addition that would otherwise pass assertion 1 unnoticed.
if(NOT "${shim_owned_exports}" STREQUAL "${owned_manifest_exports}")
    message(FATAL_ERROR
        "Shim-owned exports differ from the declared owned-exports manifest.\n"
        "Shim (owned): ${shim_owned_exports}\n"
        "Manifest: ${owned_manifest_exports}")
endif()

# IMAS-Core has no plain-C `al_plugin_begin_timerange_action` symbol despite
# its public header declaration, and calls the AOS function
# `al_begin_arraystruct_action` (without a second underscore). The shim must
# not add either alias while mirroring that surface (issues #7 and #8).
foreach(documented_omission al_plugin_begin_timerange_action al_begin_array_struct_action)
    if("${documented_omission}" IN_LIST shim_mirrored_exports)
        message(FATAL_ERROR "shim must not export ${documented_omission}")
    endif()
endforeach()
