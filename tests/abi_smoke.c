/* Links a C translation unit against the cargo-c output using only the
 * generated header. If this builds and runs, the ABI pipeline is intact:
 * cbindgen emitted the header, cargo-c produced a linkable shared library,
 * and the struct layout agrees on both sides. */

#include <assert.h>
#include <stdio.h>
#include <string.h>

#include <imas_mvdd_loader.h>

int main(void) {
    const char *version = imas_mvdd_loader_version();
    assert(version != NULL && version[0] != '\0');

    al_status_t status;
    status.code = 42;
    memset(status.message, 'x', sizeof status.message);

    imas_mvdd_loader_status_clear(&status);
    assert(status.code == 0);
    assert(status.message[0] == '\0');

    printf("imas-mvdd-loader %s: ABI smoke test passed\n", version);
    return 0;
}
