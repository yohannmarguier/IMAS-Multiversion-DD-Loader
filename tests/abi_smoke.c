/* Links a C translation unit against the cargo-c output using only the
 * generated header. If this builds and runs, the ABI pipeline is intact:
 * cbindgen emitted the header, cargo-c produced a linkable shared library,
 * and real IMAS-Core ABI symbols forward through it. */

#include <assert.h>
#include <stdio.h>
#include <string.h>

#include <imas_mvdd_loader.h>

int main(void) {
    const char *version = getALVersion();
    assert(version != NULL && version[0] != '\0');
    assert(getDDVersion() != NULL);
    assert(const2str(12345) != NULL);
    assert(err2str(-44) != NULL);

    printf("IMAS-Core %s through imas-mvdd-loader: ABI smoke test passed\n", version);
    return 0;
}
