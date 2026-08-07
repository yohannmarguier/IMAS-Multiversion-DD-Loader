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
        string(REGEX REPLACE "^.*[ \\t]" "" candidate "${line}")
        string(REGEX MATCH "^_?(al_[A-Za-z0-9_]+|const2str|err2str|getALVersion|getDDVersion)$"
            matched_symbol "${candidate}")
        if(matched_symbol)
            set(symbol "${CMAKE_MATCH_1}")
            list(APPEND exports "${symbol}")
        endif()
    endforeach()
    list(REMOVE_DUPLICATES exports)
    list(SORT exports)
    set("${output_variable}" "${exports}" PARENT_SCOPE)
endfunction()

public_abi_exports("${CORE_LIBRARY}" core_exports)
public_abi_exports("${SHIM_LIBRARY}" shim_exports)

if(NOT "${core_exports}" STREQUAL "${shim_exports}")
    message(FATAL_ERROR
        "IMAS-Core public C exports differ from the shim.\n"
        "Core: ${core_exports}\n"
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
