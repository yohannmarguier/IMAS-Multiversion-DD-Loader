#define _GNU_SOURCE

#include <dlfcn.h>
#include <limits.h>

#include "../support/shim_test_support.h"

int main(int argc, char **argv) {
    Dl_info information;
    char expected[PATH_MAX];
    char observed[PATH_MAX];

    if (argc != 2) {
        fprintf(stderr, "usage: %s <expected-shim-library>\n", argv[0]);
        return 2;
    }
    if (dladdr((void *)al_context_info, &information) == 0 || information.dli_fname == NULL) {
        fprintf(stderr, "dladdr could not identify the library providing al_context_info\n");
        return 1;
    }
    if (realpath(argv[1], expected) == NULL) {
        perror("realpath(expected shim)");
        return 1;
    }
    if (realpath(information.dli_fname, observed) == NULL) {
        perror("realpath(loaded shim)");
        return 1;
    }
    if (strcmp(expected, observed) != 0) {
        fprintf(stderr, "expected shim %s, but al_context_info came from %s\n", expected, observed);
        return 1;
    }

    puts(observed);
    return 0;
}
