/* Filesystem helpers shared by real-IMAS-Core fixture tests. */

#ifndef IMAS_MVDD_REAL_CORE_FIXTURE_SUPPORT_H
#define IMAS_MVDD_REAL_CORE_FIXTURE_SUPPORT_H

#include <errno.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#include <hdf5.h>
#include "shim_test_support.h"

typedef struct {
    char temp_dir[1024];
    char pulse_dir[1024];
    int is_live;
} real_core_fixture_copy;

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

static inline void remove_real_core_fixture_copy(real_core_fixture_copy *fixture) {
    if (!fixture->is_live) {
        return;
    }
    if (fixture->pulse_dir[0] != '\0') {
        remove_fixture_file(fixture->pulse_dir, "equilibrium.h5");
        remove_fixture_file(fixture->pulse_dir, "master.h5");
        CHECK(rmdir(fixture->pulse_dir) == 0 || errno == ENOENT);
    }
    CHECK(rmdir(fixture->temp_dir) == 0 || errno == ENOENT);
    fixture->is_live = 0;
}

static inline void create_real_core_fixture_copy(real_core_fixture_copy *fixture,
                                                 const char *fixture_directory,
                                                 const char *dd_version,
                                                 const char *temp_template) {
    int temp_length = snprintf(fixture->temp_dir, sizeof fixture->temp_dir, "%s", temp_template);
    CHECK(temp_length > 0 && (size_t)temp_length < sizeof fixture->temp_dir);
    CHECK(mkdtemp(fixture->temp_dir) != NULL);
    fixture->is_live = 1;
    int pulse_length = snprintf(fixture->pulse_dir, sizeof fixture->pulse_dir, "%s/dd-%s",
                                fixture->temp_dir, dd_version);
    CHECK(pulse_length > 0 && (size_t)pulse_length < sizeof fixture->pulse_dir);
    CHECK(mkdir(fixture->pulse_dir, 0700) == 0);
    static const char *const files[] = {"equilibrium.h5", "master.h5"};
    for (size_t i = 0; i < sizeof files / sizeof files[0]; ++i) {
        char source[1024];
        char copy[1024];
        int source_length = snprintf(source, sizeof source, "%s/dd-%s/%s", fixture_directory,
                                     dd_version, files[i]);
        int copy_length = snprintf(copy, sizeof copy, "%s/%s", fixture->pulse_dir, files[i]);
        CHECK(source_length > 0 && (size_t)source_length < sizeof source);
        CHECK(copy_length > 0 && (size_t)copy_length < sizeof copy);
        copy_fixture_file(source, copy);
    }
}

static inline void real_core_fixture_ids_file(const real_core_fixture_copy *fixture, char *path,
                                              size_t path_size) {
    int length = snprintf(path, path_size, "%s/equilibrium.h5", fixture->pulse_dir);
    CHECK(length > 0 && (size_t)length < path_size);
}

/* The DD-version stamp is a scalar UTF-8 variable-length string. The caller
 * owns only its fixed output buffer; HDF5 allocates and releases `stored`. */
static inline void read_fixture_dd_version_stamp(const char *ids_file, char *version,
                                                 size_t version_size) {
    hid_t file = H5Fopen(ids_file, H5F_ACC_RDONLY, H5P_DEFAULT);
    CHECK(file >= 0);
    hid_t dataset = H5Dopen2(file, "/equilibrium/ids_properties&version_put&data_dictionary",
                             H5P_DEFAULT);
    CHECK(dataset >= 0);
    hid_t datatype = H5Dget_type(dataset);
    CHECK(datatype >= 0);
    CHECK(H5Tget_class(datatype) == H5T_STRING);
    CHECK(H5Tis_variable_str(datatype) > 0);
    char *stored = NULL;
    CHECK(H5Dread(dataset, datatype, H5S_ALL, H5S_ALL, H5P_DEFAULT, &stored) >= 0);
    CHECK(stored != NULL);
    int length = snprintf(version, version_size, "%s", stored);
    CHECK(length >= 0 && (size_t)length < version_size);
    CHECK(H5free_memory(stored) >= 0);
    CHECK(H5Tclose(datatype) >= 0);
    CHECK(H5Dclose(dataset) >= 0);
    CHECK(H5Fclose(file) >= 0);
}

#endif
