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

# The key is supplied explicitly so this check needs neither a real PDB nor a
# build; dev computes it from the symbol file via scripts/pdb_identity.py.
retain() { # key [sha]
  CODETERM_DEV_PDB_DIR="$store" ./build.sh retain-pdb "${2:-deadbee}" "$src" "$1" >/dev/null
}

# The copy keeps the linker's own file name — Windows symbol lookup finds a PDB by
# the name in the binary's RSDS record, so a renamed file would never be found.
retain aaaaaaaaaaaa b9ba09cf3
if [ ! -f "$store/aaaaaaaaaaaa/codeterm.pdb" ]; then
  echo "retain-pdb must store the symbol file at <store>/<key>/codeterm.pdb" >&2
  find "$store" >&2 || true
  exit 1
fi
if [ "$(cat "$store/aaaaaaaaaaaa/codeterm.pdb")" != "pdb-for-build-one" ]; then
  echo "retained symbol file does not match the linked one" >&2
  exit 1
fi

# The build sha is recorded beside the symbols, informative only.
if [ "$(cat "$store/aaaaaaaaaaaa/build-sha.txt")" != "b9ba09cf3" ]; then
  echo "retain-pdb must record the build sha beside the retained symbols" >&2
  exit 1
fi

# A rerun on the same key must not fork a second copy.
retain aaaaaaaaaaaa b9ba09cf3
retained_for_key="$(find "$store/aaaaaaaaaaaa" -name '*.pdb' | wc -l | tr -d ' ')"
if [ "$retained_for_key" != "1" ]; then
  echo "a repeated retain must not duplicate the symbol file (found $retained_for_key)" >&2
  exit 1
fi

# No partial file is ever left behind for a reader to mistake for symbols.
if find "$store" -name '*.partial' | grep -q .; then
  echo "retain-pdb left a partial copy in the store" >&2
  exit 1
fi

# A fourth distinct build evicts the oldest; exactly three are kept.
for key in bbbbbbbbbbbb cccccccccccc dddddddddddd; do
  printf 'pdb-for-%s' "$key" > "$src"
  retain "$key"
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
for key in bbbbbbbbbbbb cccccccccccc dddddddddddd; do
  if [ ! -f "$store/$key/codeterm.pdb" ]; then
    echo "pruning removed a build that is still within the retention count: $key" >&2
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

# Without a key and with an unreadable symbol file, nothing is filed under a guess.
unkeyed_store="$fixture/pdb-unkeyed"
CODETERM_DEV_PDB_DIR="$unkeyed_store" ./build.sh retain-pdb b9ba09cf3 "$src" >/dev/null 2>&1 || true
if find "$unkeyed_store" -name 'codeterm.pdb' 2>/dev/null | grep -q .; then
  echo "a file with no readable debug identity must not be retained under a guessed key" >&2
  exit 1
fi

# The key comes from the symbol file's own identity, never from a commit sha.
if ! grep -q 'dev_pdb_key' build.sh; then
  echo "retention must derive its key from the symbol file, not from a sha" >&2
  exit 1
fi
key_from_helper="$(python scripts/pdb_identity.py build.sh 2>/dev/null || true)"
if [ -n "$key_from_helper" ]; then
  echo "pdb_identity must report no identity for a non-PE, non-PDB file" >&2
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
