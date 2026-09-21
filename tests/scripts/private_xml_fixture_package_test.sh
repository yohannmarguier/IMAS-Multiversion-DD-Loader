#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
test_root=$(mktemp -d "${TMPDIR:-/tmp}/imas-mvdd-private-xml-package.XXXXXX")
trap 'rm -rf "$test_root"' EXIT

production_prefix="$test_root/production"
xml_stage_directory="$test_root/xml-stage/lib"
xml_stage_library="$xml_stage_directory/libimas_mvdd_loader.so"
fixture_prefix="$test_root/private-fixture"

mkdir -p "$production_prefix/lib/cmake/imas-mvdd-loader" "$xml_stage_directory"
for alias in libimas_mvdd_loader.so libimas_mvdd_loader.so.0.1; do
  printf '%s\n' production > "$production_prefix/lib/$alias"
done
printf '%s\n' production > "$production_prefix/lib/libimas_mvdd_loader.so.0.1.0"
printf '%s\n' package-config > \
  "$production_prefix/lib/cmake/imas-mvdd-loader/imas-mvdd-loaderConfig.cmake"
printf '%s\n' xml-fixture > "$xml_stage_directory/libimas_mvdd_loader.so.0.1.0"
ln -s libimas_mvdd_loader.so.0.1.0 "$xml_stage_library"
ln -s libimas_mvdd_loader.so.0.1.0 \
  "$xml_stage_directory/libimas_mvdd_loader.so.0.1"

bash "$repo_root/scripts/prepare-private-xml-fixture-package.sh" \
  "$production_prefix" "$xml_stage_library" "$fixture_prefix"

for alias in libimas_mvdd_loader.so libimas_mvdd_loader.so.0.1 \
    libimas_mvdd_loader.so.0.1.0; do
  test "$(cat "$production_prefix/lib/$alias")" = production
  test "$(cat "$fixture_prefix/lib/$alias")" = xml-fixture
done
cmp \
  "$production_prefix/lib/cmake/imas-mvdd-loader/imas-mvdd-loaderConfig.cmake" \
  "$fixture_prefix/lib/cmake/imas-mvdd-loader/imas-mvdd-loaderConfig.cmake"

if bash "$repo_root/scripts/prepare-private-xml-fixture-package.sh" \
    "$production_prefix" "$xml_stage_library" "$fixture_prefix"; then
  echo "private XML fixture package unexpectedly overwrote an existing prefix" >&2
  exit 1
fi
