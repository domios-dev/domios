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

# 6) Language / tool caches. Prefer OFFICIAL prune/verify (they only drop unreferenced/old);
#    for caches with no official pruner, remove dirs untouched > KEEP_DAYS. NEVER touch app
#    STATE under ~/Library/Application Support or ~/Library/org.swift.swiftpm — only
#    regenerable download/build caches. Aggressive variants are opt-in via env.
prune_stale_dir() {  # $1=cache root: rm immediate children untouched > KEEP_DAYS
  [[ -d "$1" ]] || return 0
  echo "[cache] $1 ($(sz "$1"))"
  while IFS= read -r old; do run "rm -rf '$old'"; done \
    < <(find "$1" -mindepth 1 -maxdepth 1 -mtime +"$KEEP_DAYS" 2>/dev/null)
}
command -v pip3   >/dev/null 2>&1 && { echo "[pip] $(pip3 cache dir 2>/dev/null)"; run "pip3 cache purge >/dev/null 2>&1 || true"; }
command -v poetry >/dev/null 2>&1 && run "poetry cache clear --all PyPI -n >/dev/null 2>&1 || true"
command -v go     >/dev/null 2>&1 && { echo "[go] build cache $(sz "$(go env GOCACHE 2>/dev/null)")"; run "go clean -cache >/dev/null 2>&1 || true"; [[ "${GO_MODCACHE_CLEAN:-0}" == 1 ]] && run "go clean -modcache >/dev/null 2>&1 || true"; }
command -v brew   >/dev/null 2>&1 && { echo "[brew] cache $(sz "$(brew --cache 2>/dev/null)")"; run "brew cleanup -s >/dev/null 2>&1 || true"; [[ "${BREW_AUTOREMOVE:-0}" == 1 ]] && run "brew autoremove >/dev/null 2>&1 || true"; }
prune_stale_dir "$HOME/Library/Caches/org.swift.swiftpm"   # SwiftPM repo/clone cache (NOT ~/Library/org.swift.swiftpm state)
prune_stale_dir "$HOME/Library/Caches/node-gyp"            # per-node-version C++ headers
prune_stale_dir "$HOME/.cache/node-gyp"
prune_stale_dir "$HOME/.cache/puppeteer"                   # old chrome revisions (separate from Playwright)
prune_stale_dir "$HOME/.cache/typescript"                  # tsc fetched lib cache

echo "free after:  $(avail)"
[[ $APPLY -eq 0 ]] && echo ">>> DRY-RUN — re-run with --apply to reclaim."
exit 0
