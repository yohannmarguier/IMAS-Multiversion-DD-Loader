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

# Copy the installed package metadata first, then replace only its library
# payload with the crate's private xml-fixture-source build. The production
# prefix stays untouched, and no installed option can select this source.
cmake -E copy_directory "$production_prefix" "$fixture_prefix"
fixture_library="$fixture_prefix/lib/$(basename "$xml_fixture_library")"
cmake -E copy_if_different "$xml_fixture_library" "$fixture_library"

if ! cmp -s "$xml_fixture_library" "$fixture_library"; then
  echo "private XML fixture library was not staged into $fixture_prefix" >&2
  exit 1
fi

printf 'Private XML fixture package: %s\n' "$fixture_prefix"
