#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
test_root=$(mktemp -d "${TMPDIR:-/tmp}/imas-mvdd-private-xml-package.XXXXXX")
trap 'rm -rf "$test_root"' EXIT

production_prefix="$test_root/production"
xml_stage_library="$test_root/xml-stage/lib/libimas_mvdd_loader.so"
fixture_prefix="$test_root/private-fixture"

mkdir -p "$production_prefix/lib/cmake/imas-mvdd-loader" "$(dirname "$xml_stage_library")"
printf '%s\n' production > "$production_prefix/lib/libimas_mvdd_loader.so"
printf '%s\n' package-config > \
  "$production_prefix/lib/cmake/imas-mvdd-loader/imas-mvdd-loaderConfig.cmake"
printf '%s\n' xml-fixture > "$xml_stage_library"

bash "$repo_root/scripts/prepare-private-xml-fixture-package.sh" \
  "$production_prefix" "$xml_stage_library" "$fixture_prefix"

test "$(cat "$production_prefix/lib/libimas_mvdd_loader.so")" = production
test "$(cat "$fixture_prefix/lib/libimas_mvdd_loader.so")" = xml-fixture
cmp \
  "$production_prefix/lib/cmake/imas-mvdd-loader/imas-mvdd-loaderConfig.cmake" \
  "$fixture_prefix/lib/cmake/imas-mvdd-loader/imas-mvdd-loaderConfig.cmake"

if bash "$repo_root/scripts/prepare-private-xml-fixture-package.sh" \
    "$production_prefix" "$xml_stage_library" "$fixture_prefix"; then
  echo "private XML fixture package unexpectedly overwrote an existing prefix" >&2
  exit 1
fi
