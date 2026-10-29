#!/usr/bin/env bash
set +x
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
CONTAINER_ID=''
cleanup() {
  local result=$?
  trap - EXIT
  if ! "$MEM_PARITY_PYTHON" "$ROOT/evals/mem-backend-parity/container.py" --check-artifacts "$OUT_DIR"; then
    result=2
  fi
  if [ -n "$CONTAINER_ID" ]; then
    docker rm --force "$CONTAINER_ID" >/dev/null || result=2
  fi
  unset POSTGRES_PASSWORD MEM_PARITY_POSTGRES_URL
  exit "$result"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
POSTGRES_PASSWORD="$("$MEM_PARITY_PYTHON" -c 'import secrets; print(secrets.token_urlsafe(24))')"
export POSTGRES_PASSWORD
export MEM_PARITY_POSTGRES_URL="$("$MEM_PARITY_PYTHON" -c 'import os; print("postgresql://" + "domios_parity:" + os.environ["POSTGRES_PASSWORD"] + "@127.0.0.1:55439/domios_parity")')"
CONTAINER_ID="$(MSYS_NO_PATHCONV=1 docker run --detach --name domios-mem-backend-parity \
  --label domios.bench=mem-backend-parity --env POSTGRES_DB=domios_parity \
  --env POSTGRES_USER=domios_parity --env POSTGRES_PASSWORD \
  --publish 127.0.0.1:55439:5432 --tmpfs /var/lib/postgresql/data pgvector/pgvector:pg17)"
export MEM_PARITY_CONTAINER_ID="$CONTAINER_ID"
CARGO_ARGS=(run --locked --manifest-path src-tauri/Cargo.toml -p codeterm-mem-postgres --example mem_backend_parity)
if [ "$MODE" = local ]; then
  CARGO_ARGS+=(--features codeterm-mem/embeddings)
fi
BENCH_ARGS=(--embedder "$MODE" --out "$OUT_DIR")
[ -z "$MODEL" ] || BENCH_ARGS+=(--local-model "$MODEL")
echo "Artifacts: $OUT_DIR"
cd "$ROOT"
cargo "${CARGO_ARGS[@]}" -- "${BENCH_ARGS[@]}" 2>&1 | tee "$OUT_DIR/run.log"
