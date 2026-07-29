#!/usr/bin/env bash

set -uo pipefail

script_path="${BASH_SOURCE[0]}"
script_dir="${script_path%/*}"
[ "$script_dir" != "$script_path" ] || script_dir="."
ROOT="$(cd "$script_dir/.." && pwd)"
exec node "$ROOT/scripts/run-ts-unit-tests.mjs" "$@"
