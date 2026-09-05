#!/usr/bin/env bash
# The dev symbol store keeps the last few linked builds and prunes the rest.
#
# Without it a dev PDB survives only until the next link: the 2026-09-05 abort
# was unsymbolizable nine minutes after it was written. This drives
# `./build.sh retain-pdb` against a fixture store, so it needs no Cargo build.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT

store="$fixture/pdb"
src="$fixture/codeterm.pdb"
printf 'pdb-for-build-one' > "$src"

retain() { # sha
  CODETERM_DEV_PDB_DIR="$store" ./build.sh retain-pdb "$1" "$src" >/dev/null
}

# The copy keeps the linker's own file name — Windows symbol lookup finds a PDB by
# the name in the binary's RSDS record, so a renamed file would never be found.
retain aaaaaaaaaaaa
if [ ! -f "$store/aaaaaaaaaaaa/codeterm.pdb" ]; then
  echo "retain-pdb must store the symbol file at <store>/<sha>/codeterm.pdb" >&2
  find "$store" >&2 || true
  exit 1
fi
if [ "$(cat "$store/aaaaaaaaaaaa/codeterm.pdb")" != "pdb-for-build-one" ]; then
  echo "retained symbol file does not match the linked one" >&2
  exit 1
fi

# A rerun on the same sha must not fork a second copy.
retain aaaaaaaaaaaa
retained_for_sha="$(find "$store/aaaaaaaaaaaa" -name '*.pdb' | wc -l | tr -d ' ')"
if [ "$retained_for_sha" != "1" ]; then
  echo "a repeated retain must not duplicate the symbol file (found $retained_for_sha)" >&2
  exit 1
fi

# No partial file is ever left behind for a reader to mistake for symbols.
if find "$store" -name '*.partial' | grep -q .; then
  echo "retain-pdb left a partial copy in the store" >&2
  exit 1
fi

# A fourth distinct build evicts the oldest; exactly three are kept.
for sha in bbbbbbbbbbbb cccccccccccc dddddddddddd; do
  printf 'pdb-for-%s' "$sha" > "$src"
  retain "$sha"
done

kept="$(find "$store" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')"
if [ "$kept" != "3" ]; then
  echo "the symbol store must keep exactly 3 builds, found $kept" >&2
  find "$store" -mindepth 1 -maxdepth 1 -type d >&2
  exit 1
fi
if [ -d "$store/aaaaaaaaaaaa" ]; then
  echo "the oldest build must be pruned first" >&2
  exit 1
fi
for sha in bbbbbbbbbbbb cccccccccccc dddddddddddd; do
  if [ ! -f "$store/$sha/codeterm.pdb" ]; then
    echo "pruning removed a build that is still within the retention count: $sha" >&2
    exit 1
  fi
done

# The retention count is a constant, not a literal scattered through the script.
if ! grep -q 'DEV_PDB_KEEP_DEFAULT=3' build.sh; then
  echo "the retention count must stay a named constant in build.sh" >&2
  exit 1
fi

# A build with no PDB (any non-MSVC toolchain) is not an error.
missing_store="$fixture/pdb-missing"
CODETERM_DEV_PDB_DIR="$missing_store" ./build.sh retain-pdb eeeeeeeeeeee \
  "$fixture/does-not-exist.pdb" >/dev/null
if [ -d "$missing_store/eeeeeeeeeeee" ]; then
  echo "a build without a PDB must not create an empty store entry" >&2
  exit 1
fi

# Both dev launch paths retain: --wait once health names the running build, and
# the foreground session once the linked build has finished running.
dev_body="$(sed -n '/^  dev)/,/^  e2e)/p' build.sh)"
if ! grep -q 'retain_dev_pdb "\$SHA"' <<< "$dev_body"; then
  echo "dev --wait must retain the symbol file for the build health confirmed" >&2
  exit 1
fi
if ! grep -q 'retain_dev_pdb "\$(git rev-parse HEAD' <<< "$dev_body"; then
  echo "foreground dev must retain the symbol file for the build it ran" >&2
  exit 1
fi
tauri_line="$(grep -n 'pnpm tauri dev --no-watch' <<< "$dev_body" | cut -d: -f1 | head -n1)"
retain_line="$(grep -n 'retain_dev_pdb "\$(git rev-parse HEAD' <<< "$dev_body" | cut -d: -f1 | head -n1)"
if [ -z "$tauri_line" ] || [ -z "$retain_line" ] || [ "$retain_line" -le "$tauri_line" ]; then
  echo "foreground retention must run after the dev session, when the PDB on disk is the one that ran" >&2
  exit 1
fi

echo "build pdb retention check passed"
