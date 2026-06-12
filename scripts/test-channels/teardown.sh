#!/usr/bin/env bash
set -euo pipefail

name="${1:?usage: teardown.sh <name>}"
root="${CODETERM_TEST_CHANNEL_ROOT:-/tmp/codeterm-test-channels}"

rm -rf "$root/$name" "$root/$name.git"
