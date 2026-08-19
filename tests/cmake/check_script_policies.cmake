cmake_minimum_required(VERSION 3.21)

# Every script run with `cmake -P` takes its policy version from its own
# cmake_minimum_required and nothing else: script mode has no enclosing project
# to inherit from. Omit it and CMake 3.x leaves policies such as CMP0057
# (`IN_LIST`) unset, while CMake 4.x defaults them to NEW — so the script works
# on a modern local toolchain and dies only on CI. This guard makes that skew a
# test failure on any CMake instead of a surprise in the runner logs.
#
# Only `-P` scripts are checked. The nested CMakeLists.txt fixtures are
# configured as projects, where a missing minimum is already a loud warning.

foreach(required_variable SCRIPT_DIR)
    if(NOT DEFINED ${required_variable})
        message(FATAL_ERROR "${required_variable} must be supplied")
    endif()
endforeach()

# The floor the top-level CMakeLists.txt declares. Anything older reintroduces
# the unset-policy problem this guard exists to prevent.
set(required_policy_version 3.21)

file(GLOB_RECURSE script_files "${SCRIPT_DIR}/*.cmake")
if(NOT script_files)
    message(FATAL_ERROR
        "No CMake script files found under ${SCRIPT_DIR}; this guard would "
        "otherwise pass without checking anything")
endif()
list(SORT script_files)

# Returns the script's first line that is neither blank nor a whole-line
# comment, so a leading explanatory comment stays allowed but a command placed
# ahead of the version pin does not.
function(first_command_line script output_variable)
    file(READ "${script}" contents)
    # Protect semicolons in the file's own text from being read as list
    # separators when the content is split into lines.
    string(REPLACE ";" "\\;" contents "${contents}")
    string(REPLACE "\n" ";" lines "${contents}")
    foreach(line IN LISTS lines)
        string(STRIP "${line}" stripped_line)
        if(stripped_line STREQUAL "")
            continue()
        endif()
        string(SUBSTRING "${stripped_line}" 0 1 first_character)
        if(first_character STREQUAL "#")
            continue()
        endif()
        set("${output_variable}" "${stripped_line}" PARENT_SCOPE)
        return()
    endforeach()
    set("${output_variable}" "" PARENT_SCOPE)
endfunction()

foreach(script IN LISTS script_files)
    file(RELATIVE_PATH script_name "${SCRIPT_DIR}" "${script}")
    first_command_line("${script}" opening_command)

    if(NOT opening_command MATCHES
            "^cmake_minimum_required[ \t]*\\([ \t]*VERSION[ \t]+([0-9]+(\\.[0-9]+)*)")
        message(FATAL_ERROR
            "${script_name} must begin with cmake_minimum_required(VERSION "
            "${required_policy_version}) before any other command, so that "
            "`cmake -P` fixes its policy version. Found instead: "
            "${opening_command}")
    endif()

    set(declared_version "${CMAKE_MATCH_1}")
    if(declared_version VERSION_LESS required_policy_version)
        message(FATAL_ERROR
            "${script_name} declares policy version ${declared_version}, "
            "older than the required ${required_policy_version}; policies "
            "introduced after it would silently stay unset")
    endif()
endforeach()
