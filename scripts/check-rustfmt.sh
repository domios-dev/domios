#!/usr/bin/env bash
# Caller: build.sh fmt-check; catalogue rust-fmt (CI tests-fast.yml guards); scripts qa-gates.lib.sh
# rustfmt --check over every tracked Rust file of the src-tauri workspace and the CLI.
# cargo fmt discovers files through module trees and on Windows reported files clean
# that rustfmt itself rejects, so this lists files from git instead.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

edition=2021
scope=(src-tauri cli)
skip='^src-tauri/(pty-fork|vendor)/'

foreign=$(git ls-files -- "${scope[@]/%//**Cargo.toml}" | grep -Ev "$skip" |
  xargs grep -L "^edition = \"$edition\"" || true)
if [ -n "$foreign" ]; then
  echo "check-rustfmt: these manifests do not declare edition $edition; teach the guard their edition:" >&2
  echo "$foreign" >&2
  exit 2
fi

files=()
while IFS= read -r -d '' file; do files+=("$file"); done < <(git ls-files -z -- "${scope[@]/%//**.rs}" | grep -zEv "$skip")
# Batches stay under the Windows command-line limit; the first failing rustfmt status is returned as is.
status=0
for ((i = 0; i < ${#files[@]}; i += 200)); do
  rustfmt --edition "$edition" --check "${files[@]:i:200}" || { code=$?; [ "$status" -ne 0 ] || status=$code; }
done
[ "$status" -eq 0 ] || exit "$status"
echo "check-rustfmt: tracked Rust files are formatted"
