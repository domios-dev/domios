#!/usr/bin/env bash
# Memory recall evaluation wrapper.
#
# Usage:
#   ./run.sh              # run once, write results/report
#   ./run.sh --check      # run and fail on regression vs baseline.json
#   ./run.sh --check --baseline ./path/to/baseline.json

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(git rev-parse --show-toplevel)"
BENCHMARK_JSON="$SCRIPT_DIR/benchmark.json"
RESULT_DIR="$SCRIPT_DIR/results"
REPORT_DIR="$SCRIPT_DIR/reports"
BASELINE_PATH="$SCRIPT_DIR/baseline.json"
DECISION_MODE=off
DECISION_DATA_DIR=""
DECISION_GRAPH=0
DECISION_DOCS=0

# Regression policy (derived from observed run-to-run variance on this machine).
CHECK_MODE=0
RECALL_TOL=0.02
MRR_TOL=0.02
P50_TOL=0.40
P95_TOL=0.40
TOTAL_TOL=2.0

usage() {
  cat <<'EOF'
Usage:
  ./run.sh [--check] [--baseline PATH] [--decision-rerank off|stub|live] [--decision-data-dir PATH] [--decision-graph M] [--decision-docs M]

Options:
  --check           Compare latest mem-bench run against baseline.json.
  --baseline PATH   Baseline file for --check (default: baseline.json)
  --decision-rerank MODE  Optional A/B mode: off, stub, or live.
  --decision-data-dir PATH  Data directory containing decision-models token.
  --decision-graph M  Add a re-rank+graph arm with up to M graph candidates (needs stub or live).
  --decision-docs M  Add a re-rank+docs arm with up to M embedded sections (needs stub or live; max 10).
  -h, --help       Show this help.

Defaults:
  check tolerances:
  recall@K: ±0.02, MRR: ±0.02, p50/p95 latency: ±40%, total latency: ±200%
EOF
}

while [[ $# -gt 0 ]]; do
  case "${1:-}" in
    --check)
      CHECK_MODE=1
      shift
      ;;
    --baseline)
      shift
      BASELINE_PATH="${1:?--baseline needs path}"
      shift
      ;;
    --decision-rerank)
      shift
      DECISION_MODE="${1:?--decision-rerank needs mode}"
      shift
      ;;
    --decision-data-dir)
      shift
      DECISION_DATA_DIR="${1:?--decision-data-dir needs path}"
      shift
      ;;
    --decision-graph)
      shift
      DECISION_GRAPH="${1:?--decision-graph needs count}"
      shift
      ;;
    --decision-docs)
      shift
      DECISION_DOCS="${1:?--decision-docs needs count}"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "ERROR: unknown arg: $1"
      usage
      exit 1
      ;;
  esac
done

if [[ "$CHECK_MODE" -eq 1 && "$DECISION_MODE" != off ]]; then
  echo "ERROR: --check requires --decision-rerank off"
  exit 1
fi

if [[ "$DECISION_DOCS" != 0 && "$DECISION_MODE" == off ]]; then
  echo "ERROR: --decision-docs requires --decision-rerank stub or live"
  exit 1
fi

run() {
  local decision_args=(--decision-rerank "$DECISION_MODE")
  if [[ -n "$DECISION_DATA_DIR" ]]; then
    decision_args+=(--decision-data-dir "$DECISION_DATA_DIR")
  fi
  if [[ "$DECISION_GRAPH" != 0 ]]; then
    decision_args+=(--decision-graph "$DECISION_GRAPH")
  fi
  if [[ "$DECISION_DOCS" != 0 ]]; then
    decision_args+=(--decision-docs "$DECISION_DOCS")
  fi
  (
    cd "$ROOT_DIR/src-tauri"
    cargo run -p codeterm --manifest-path "$ROOT_DIR/src-tauri/Cargo.toml" --bin mem-bench --release -- \
      --fixture "$BENCHMARK_JSON" \
      --output-dir "$RESULT_DIR" \
      --report-dir "$REPORT_DIR" \
      "${decision_args[@]}"
  )
}

run

shopt -s nullglob
json_files=("$RESULT_DIR"/*.json)
if [[ ${#json_files[@]} -eq 0 ]]; then
  echo "ERROR: no results produced in $RESULT_DIR"
  exit 1
fi
LATEST_JSON="${json_files[${#json_files[@]}-1]}"
printf 'latest: %s\n' "$LATEST_JSON"

if [[ "$CHECK_MODE" -ne 1 ]]; then
  exit 0
fi

if [[ ! -f "$BASELINE_PATH" ]]; then
  echo "ERROR: baseline not found: $BASELINE_PATH"
  echo "Create it by running without --check and writing:
  cp $LATEST_JSON $BASELINE_PATH (or update evals/mem-recall/baseline.json)."
  exit 1
fi

python3 - "$LATEST_JSON" "$BASELINE_PATH" \
  "$RECALL_TOL" "$MRR_TOL" "$P50_TOL" "$P95_TOL" "$TOTAL_TOL" <<'PY'
import json
import sys

new_path, base_path, recall_tol, mrr_tol, p50_tol, p95_tol, total_tol = sys.argv[1:8]

def read_report(path):
    with open(path, "r", encoding="utf-8") as f:
        return json.load(f)

def compare(name, mode, got, base, tol):
    if mode == "higher":
        delta = got - base
        ok = delta + tol >= 0
    elif mode == "relative_lower":
        limit = base * (1.0 + tol)
        ok = got <= limit
    else:
        raise ValueError(mode)
    return ok, delta if mode == "higher" else got - base

new = read_report(new_path)
base = read_report(base_path)

checks = [
    ("recall@1", "higher", new["metrics"]["recall_at_1"], base["metrics"]["recall_at_1"], float(recall_tol)),
    ("recall@5", "higher", new["metrics"]["recall_at_5"], base["metrics"]["recall_at_5"], float(recall_tol)),
    ("recall@10", "higher", new["metrics"]["recall_at_10"], base["metrics"]["recall_at_10"], float(recall_tol)),
    ("mrr", "higher", new["metrics"]["mrr"], base["metrics"]["mrr"], float(mrr_tol)),
    ("p50_ms", "relative_lower", new["metrics"]["p50_latency_ms"], base["metrics"]["p50_latency_ms"], float(p50_tol)),
    ("p95_ms", "relative_lower", new["metrics"]["p95_latency_ms"], base["metrics"]["p95_latency_ms"], float(p95_tol)),
    ("total_ms", "relative_lower", new["metrics"]["total_wall_ms"], base["metrics"]["total_wall_ms"], float(total_tol)),
]

failed = []
for name, mode, got, base_value, tol in checks:
    ok, delta = compare(name, mode, got, base_value, tol)
    if not ok:
        failed.append((name, got, base_value, delta, tol))

print(f"baseline commit: {base.get('commit','unknown')} / run {base.get('timestamp','unknown')}")
print(f"current commit: {new.get('commit','unknown')} / run {new.get('timestamp','unknown')}")

if failed:
    print("FAIL: mem-recall regression beyond tolerance")
    for name, got, base_value, delta, tol in failed:
        if name in {"p50_ms", "p95_ms", "total_ms"}:
            tolerance_desc = f"+{tol*100:.0f}% over baseline"
        else:
            tolerance_desc = f"±{tol}"
        print(f"  - {name}: {got:.6f} vs {base_value:.6f} (delta {delta:.6f}; tol {tolerance_desc})")
    sys.exit(1)

print("PASS: mem-recall metrics within tolerance")
PY
