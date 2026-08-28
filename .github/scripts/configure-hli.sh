#!/usr/bin/env bash
# Configure the IMAS-Fortran HLI against the installed shim.
#
# Extracted from the workflow's inline step so the build step can call it again
# after discarding a restored build tree it could not build from. The arguments
# are unchanged from that step; see the comments at its call site in
# .github/workflows/hli-validation.yml for why each one is here.
set -euo pipefail

: "${HLI_DD_VERSION:?}" "${GITHUB_WORKSPACE:?}"

cmake -B build \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_C_COMPILER=gcc-14 \
  -DCMAKE_CXX_COMPILER=g++-14 \
  -DCMAKE_Fortran_COMPILER=gfortran-14 \
  -DCMAKE_CXX_STANDARD=17 \
  -DAL_USE_MULTIVERSION_SHIM=ON \
  -DCMAKE_PREFIX_PATH="$GITHUB_WORKSPACE/dist" \
  -DDD_VERSION="${HLI_DD_VERSION}" \
  -DAL_BACKEND_HDF5=ON \
  -DAL_BACKEND_MDSPLUS=OFF \
  -DAL_BACKEND_UDA=OFF \
  -DAL_BUILD_MDSPLUS_MODELS=OFF \
  -DAL_TESTS=ON \
  -DAL_EXAMPLES=ON \
  -DAL_PLAYGROUND=ON \
  -DAL_PLUGINS=OFF \
  -DAL_HLI_DOCS=OFF
