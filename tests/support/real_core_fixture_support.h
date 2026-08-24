/* Filesystem helpers shared by real-IMAS-Core fixture tests. */

#ifndef IMAS_MVDD_REAL_CORE_FIXTURE_SUPPORT_H
#define IMAS_MVDD_REAL_CORE_FIXTURE_SUPPORT_H

#include <errno.h>
#include <stdio.h>
#include <unistd.h>

#include "shim_test_support.h"

static inline void copy_fixture_file(const char *from, const char *to) {
    FILE *in = fopen(from, "rb");
    CHECK(in != NULL);
    FILE *out = fopen(to, "wb");
    CHECK(out != NULL);
    char buffer[65536];
    size_t read_bytes;
    while ((read_bytes = fread(buffer, 1, sizeof buffer, in)) > 0) {
        CHECK(fwrite(buffer, 1, read_bytes, out) == read_bytes);
    }
    CHECK(ferror(in) == 0);
    CHECK(fclose(out) == 0);
    CHECK(fclose(in) == 0);
}

static inline void remove_fixture_file(const char *directory, const char *name) {
    char path[1024];
    int length = snprintf(path, sizeof path, "%s/%s", directory, name);
    CHECK(length > 0 && (size_t)length < sizeof path);
    CHECK(unlink(path) == 0 || errno == ENOENT);
}

#endif
