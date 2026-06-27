#!/usr/bin/env bash
# Reclaim disk from Rust/Tauri build caches + Docker — safe by design.
#
# Why this exists: cargo has NO garbage collector for per-project target/ dirs.
# Every dev restart, branch switch and toolchain bump leaves stale units in
# target/debug/{deps,incremental}; over months a long-lived worktree balloons to
# 100GB+. incremental/ is a pure rebuild-speed cache (always safe to delete);
# deps/ is pruned by mtime with cargo-sweep so artifacts for code you still touch
# survive. Docker build caches (release machine) grow unbounded too.
#
# NEVER deletes source, .git, node_modules, or prod data. Only build caches.
#
# Usage:
#   clean-build-caches.sh            # dry-run: report what WOULD be freed
#   clean-build-caches.sh --apply    # actually clean
#   KEEP_DAYS=10 clean-build-caches.sh --apply
#   DEV_ROOT=~/Developer clean-build-caches.sh --apply

set -uo pipefail

APPLY=0
[[ "${1:-}" == "--apply" ]] && APPLY=1
KEEP_DAYS="${KEEP_DAYS:-10}"          # keep artifacts touched within N days
ORPHAN_DAYS="${ORPHAN_DAYS:-3}"       # an orphan target untouched this long is dead
DEV_ROOT="${DEV_ROOT:-$HOME/Developer}"

run() { if [[ $APPLY -eq 1 ]]; then eval "$@"; else echo "    would: $*"; fi; }
sz() { du -sh "$1" 2>/dev/null | cut -f1; }
avail() { df -h / | awk 'NR==2{print $4}'; }

echo "=== build-cache cleanup ($([ $APPLY -eq 1 ] && echo APPLY || echo DRY-RUN); keep ${KEEP_DAYS}d) ==="
echo "free before: $(avail)"

# 1) cargo-sweep across every Rust target/ under DEV_ROOT (prunes stale deps,
#    keeps artifacts you still touch). incremental/ is wiped outright — it only
#    speeds rebuilds and is regenerated on demand.
if ! command -v cargo-sweep >/dev/null 2>&1; then
  echo "[*] cargo-sweep missing"
  run "cargo install cargo-sweep"
fi
while IFS= read -r tgt; do
  [[ -f "$tgt/CACHEDIR.TAG" || -f "$tgt/.rustc_info.json" ]] || continue
  proj="$(dirname "$tgt")"
  echo "[target] $tgt ($(sz "$tgt"))"
  if command -v cargo-sweep >/dev/null 2>&1; then
    run "cargo sweep --time $KEEP_DAYS '$proj' >/dev/null 2>&1 || cargo sweep --time $KEEP_DAYS --target-dir '$tgt' >/dev/null 2>&1 || true"
  fi
  [[ -d "$tgt/debug/incremental" ]] && { echo "    incremental: $(sz "$tgt/debug/incremental")"; run "rm -rf '$tgt/debug/incremental'"; }
done < <(find "$DEV_ROOT" -maxdepth 4 -type d -name target 2>/dev/null)

# 2) Orphaned target dirs (eval harnesses set their own CARGO_TARGET_DIR and leave
#    it behind, e.g. .psetup-target). A dir that looks like a target, is NOT inside
#    a checked-out worktree's src-tauri, and is untouched for ORPHAN_DAYS = dead.
while IFS= read -r info; do
  d="$(dirname "$info")"
  case "$d" in *"/src-tauri/target"|*"/target") continue;; esac   # active project targets handled above
  if [[ -z "$(find "$d" -maxdepth 0 -mtime -"$ORPHAN_DAYS" 2>/dev/null)" ]]; then
    echo "[orphan] $d ($(sz "$d"), stale >${ORPHAN_DAYS}d)"
    run "rm -rf '$d'"
  fi
done < <(find "$DEV_ROOT" -maxdepth 2 -name .rustc_info.json 2>/dev/null)

# 3) Docker: build cache + dangling images/containers (release machine bloat).
if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
  echo "[docker] before: $(docker system df --format '{{.Type}} {{.Size}}' 2>/dev/null | tr '\n' ' ')"
  run "docker buildx prune -af >/dev/null 2>&1 || true"
  run "docker system prune -af >/dev/null 2>&1 || true"
else
  echo "[docker] daemon not reachable — skipped"
fi

# 4) cargo global registry/git cache GC (cargo 1.78+ does this automatically, but
#    trim source caches explicitly for older state).
run "cargo cache --autoclean >/dev/null 2>&1 || true"

# 5) Node / browser caches. pnpm store prune + npm cache verify are purpose-built and
#    safe (remove only unreferenced/stale). Playwright auto-GCs unused browser versions
#    on its next install (PLAYWRIGHT_SKIP_BROWSER_GC must stay unset) — so we trigger that
#    GC rather than blindly rm version dirs a pinned project might still need.
if command -v pnpm >/dev/null 2>&1; then echo "[pnpm] store ($(du -sh "$(pnpm store path 2>/dev/null)" 2>/dev/null | cut -f1))"; run "pnpm store prune >/dev/null 2>&1 || true"; fi
if command -v npm  >/dev/null 2>&1; then echo "[npm] cache ($(sz ~/.npm))"; run "npm cache verify >/dev/null 2>&1 || true"; fi
# Playwright self-GCs unused browser versions on its next `playwright install` (don't set
#    PLAYWRIGHT_SKIP_BROWSER_GC). Auto-running install from cron risks pulling a newer version,
#    so we only report; a dev test run after a version bump reclaims old browsers for free.
PW="$HOME/Library/Caches/ms-playwright"
[[ -d "$PW" ]] && echo "[playwright] browsers $(sz "$PW") (self-GC on next 'npx playwright install' in a project)"

echo "free after:  $(avail)"
[[ $APPLY -eq 0 ]] && echo ">>> DRY-RUN — re-run with --apply to reclaim."
exit 0
