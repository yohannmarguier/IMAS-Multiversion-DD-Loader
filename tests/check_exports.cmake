# Compare the public C exports mechanically rather than maintaining a
# second handwritten manifest. Both nm variants used by CI are accepted:
# Mach-O prepends an underscore to C names, ELF does not.

foreach(required_variable CORE_LIBRARY SHIM_LIBRARY NM_EXECUTABLE)
    if(NOT DEFINED ${required_variable})
        message(FATAL_ERROR "${required_variable} must be supplied")
    endif()
endforeach()

function(public_abi_exports library output_variable)
    execute_process(
        COMMAND "${NM_EXECUTABLE}" -g "${library}"
        RESULT_VARIABLE nm_result
        OUTPUT_VARIABLE nm_output
        ERROR_VARIABLE nm_error)
    if(NOT nm_result EQUAL 0)
        message(FATAL_ERROR "nm failed for ${library}: ${nm_error}")
    endif()

    string(REPLACE "\n" ";" nm_lines "${nm_output}")
    set(exports)
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

public_abi_exports("${CORE_LIBRARY}" core_exports)
public_abi_exports("${SHIM_LIBRARY}" shim_exports)

# Keep the signature checker's X-macro manifest tied to the real exported
# surface. The strict entry shape makes a malformed or multiline symbol entry
# fail here instead of silently disappearing from signature coverage.
file(STRINGS "${CMAKE_CURRENT_LIST_DIR}/abi_symbols.def" manifest_lines
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

if(NOT "${core_exports}" STREQUAL "${shim_exports}")
    message(FATAL_ERROR
        "IMAS-Core public C exports differ from the shim.\n"
        "Core: ${core_exports}\n"
        "Shim: ${shim_exports}")
endif()

if(NOT "${manifest_exports}" STREQUAL "${shim_exports}")
    message(FATAL_ERROR
        "ABI signature manifest differs from the shim exports.\n"
        "Manifest: ${manifest_exports}\n"
        "Shim: ${shim_exports}")
endif()

# IMAS-Core has no plain-C `al_plugin_begin_timerange_action` symbol despite
# its public header declaration, and calls the AOS function
# `al_begin_arraystruct_action` (without a second underscore). The shim must
# not add either alias while mirroring that surface (issues #7 and #8).
foreach(documented_omission al_plugin_begin_timerange_action al_begin_array_struct_action)
    if("${documented_omission}" IN_LIST shim_exports)
        message(FATAL_ERROR "shim must not export ${documented_omission}")
    endif()
endforeach()
