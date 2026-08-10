/* Calls the shim through the CMake package config's imported target, using
 * only what `find_package(imas-mvdd-loader)` and the installed header
 * expose — no path back into this repo's source or build tree. */

#include <assert.h>
#include <stdio.h>

#include <imas_mvdd_loader.h>

int main(void) {
    const char *version = getALVersion();
    assert(version != NULL && version[0] != '\0');
    printf("IMAS-Core %s through the installed shim: consumer smoke test passed\n", version);
    return 0;
}
