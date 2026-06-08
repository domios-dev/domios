#!/usr/bin/env bash
# Usage: run-suite.sh [provider1,provider2,...] [case-glob]
# Runs each case across providers; providers parallel per case, cases sequential.
set -euo pipefail
HARNESS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=/dev/null
source "$HARNESS_DIR/lib.sh"

PROVIDERS="${1:-claude,codex,opencode}"
GLOB="${2:-$HARNESS_DIR/cases/*.yaml}"
RUNS_DIR="$HARNESS_DIR/runs/$(date +%Y%m%d-%H%M%S)"
mkdir -p "$RUNS_DIR"
echo "runs → $RUNS_DIR"

cleanup_bench_ws   # sweep leftovers from any prior interrupted run
trap cleanup_bench_ws EXIT   # and again on exit (incl. agent-created workspaces)

for cf in $GLOB; do
  mapfile -t ids < <(python3 -c 'import sys,yaml; [print(c["id"]) for c in (yaml.safe_load(open(sys.argv[1])) or [])]' "$cf")
  for id in "${ids[@]}"; do
    pids=()
    IFS=',' read -ra PROVS <<< "$PROVIDERS"
    for prov in "${PROVS[@]}"; do
      "$HARNESS_DIR/run-case.sh" "$cf" "$id" "$prov" "$RUNS_DIR" &
      pids+=($!)
    done
    for p in "${pids[@]}"; do wait "$p" || true; done
    echo "done: $id"
  done
done
echo "suite complete → $RUNS_DIR"
