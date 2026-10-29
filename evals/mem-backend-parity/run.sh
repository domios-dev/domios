#!/usr/bin/env bash
set -euo pipefail

echo 'Default vectors are synthetic (normalized hashed-token vectors): storage and ranking parity, not neural retrieval quality.'
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$ROOT/evals/_lib/lib.sh"

MODE=synthetic
MODEL=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    --embedder)
      [ "$#" -ge 2 ] || { echo '--embedder needs synthetic or local' >&2; exit 2; }
      MODE="$2"; shift 2 ;;
    --local-model)
      [ "$#" -ge 2 ] || { echo '--local-model needs a model id' >&2; exit 2; }
      MODEL="$2"; shift 2 ;;
    *) echo "Unknown argument: $1" >&2; exit 2 ;;
  esac
done
case "$MODE" in synthetic|local) ;; *) echo "Unknown embedder: $MODE" >&2; exit 2 ;; esac
[ "$MODE" = local ] || [ -z "$MODEL" ] || { echo '--local-model requires --embedder local' >&2; exit 2; }

MEM_PARITY_PYTHON="$( { python3 -c 'import sys; sys.stdout.write(sys.executable)' || python -c 'import sys; sys.stdout.write(sys.executable)'; } 2>/dev/null)" || MEM_PARITY_PYTHON=
[ -n "$MEM_PARITY_PYTHON" ] || { echo 'Python interpreter executable is required' >&2; exit 2; }
export MEM_PARITY_PYTHON

OUT_DIR="$(mktemp -d "$EVAL_LIB_OUT_BASE/mem-backend-parity.XXXXXX")"
CARGO_ARGS=(run --locked --manifest-path src-tauri/Cargo.toml -p codeterm-mem-postgres --example mem_backend_parity)
if [ "$MODE" = local ]; then
  CARGO_ARGS+=(--features codeterm-mem/embeddings)
fi
BENCH_ARGS=(--embedder "$MODE" --out "$OUT_DIR")
[ -z "$MODEL" ] || BENCH_ARGS+=(--local-model "$MODEL")
echo "Artifacts: $OUT_DIR"
cd "$ROOT"
cargo "${CARGO_ARGS[@]}" -- "${BENCH_ARGS[@]}" 2>&1 | tee "$OUT_DIR/run.log"
