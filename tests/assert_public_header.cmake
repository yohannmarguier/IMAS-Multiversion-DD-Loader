# The generated header is the shim's installed C surface. Runtime-loader
# implementation imports belong to Rust only and must not become public API.
file(READ "${HEADER}" header_contents)

foreach(private_symbol dlopen dlsym dlerror)
    if(header_contents MATCHES "${private_symbol}[ (]")
        message(FATAL_ERROR
            "generated public header exposes private loader symbol: ${private_symbol}")
    endif()
endforeach()
