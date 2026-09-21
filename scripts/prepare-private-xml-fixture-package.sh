#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 3 ]; then
  echo "usage: $0 <production-prefix> <xml-fixture-library> <fixture-prefix>" >&2
  exit 2
fi

production_prefix=$1
xml_fixture_library=$2
fixture_prefix=$3

if [ ! -d "$production_prefix" ]; then
  echo "production package prefix does not exist: $production_prefix" >&2
  exit 1
fi
if [ ! -f "$xml_fixture_library" ]; then
  echo "private XML fixture library does not exist: $xml_fixture_library" >&2
  exit 1
fi
if [ -e "$fixture_prefix" ]; then
  echo "private XML fixture prefix already exists: $fixture_prefix" >&2
  exit 1
fi

# Copy the installed package metadata first, then replace every shared-library
# alias with the crate's private xml-fixture-source build. cargo-c installs an
# unversioned linker name, a SONAME alias and a versioned payload. Copying only
# the linker name would leave the runtime SONAME pointing at production code.
# The production prefix stays untouched, and no installed option selects this
# source.
cmake -E copy_directory "$production_prefix" "$fixture_prefix"
source_directory=$(dirname "$xml_fixture_library")
source_name=$(basename "$xml_fixture_library")
case "$source_name" in
  *.so)
    source_candidates=("$source_directory/$source_name"*)
    ;;
  *.dylib)
    source_candidates=("$source_directory/${source_name%.dylib}"*.dylib)
    ;;
  *)
    source_candidates=("$xml_fixture_library")
    ;;
esac

if [ "${#source_candidates[@]}" -eq 0 ]; then
  echo "private XML fixture library family is empty: $xml_fixture_library" >&2
  exit 1
fi

for source_library in "${source_candidates[@]}"; do
  fixture_library="$fixture_prefix/lib/$(basename "$source_library")"
  if [ ! -f "$fixture_library" ]; then
    echo "production package lacks matching library alias: $fixture_library" >&2
    exit 1
  fi
  cmake -E copy_if_different "$source_library" "$fixture_library"
  if ! cmp -s "$source_library" "$fixture_library"; then
    echo "private XML fixture library was not staged into $fixture_library" >&2
    exit 1
  fi
done

printf 'Private XML fixture package: %s\n' "$fixture_prefix"
