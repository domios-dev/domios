#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
if [[ "${CODETERM_INSTANCE:-}" != "bpdev" ]]; then
  echo "Set CODETERM_INSTANCE=bpdev before running this input fixture." >&2
  exit 2
fi
if [[ -z "${CODETERM_TAB_ID:-}" ]]; then
  echo "Run from the owner agent tab in DEV." >&2
  exit 2
fi
source "$ROOT/evals/_lib/lib.sh"
FIXTURE="$ROOT/evals/blueprint-input/quick-brown-fox.wav"
TEMPLATE="$ROOT/evals/blueprint-input/pipeline.json.in"
SPEC="$(mktemp)"
trap 'rm -f "$SPEC"' EXIT

case "${1:-}" in
  create)
    [[ -f "$FIXTURE" ]] || { echo "Missing fixture: $FIXTURE" >&2; exit 1; }
    size="$(wc -c <"$FIXTURE" | tr -d ' ')"
    (( size <= 409600 )) || { echo "Fixture exceeds 400 KB: $size bytes" >&2; exit 1; }
    python3 - "$TEMPLATE" "$SPEC" "$FIXTURE" <<'PY'
import json, pathlib, sys
template, output, fixture = map(pathlib.Path, sys.argv[1:])
spec = json.loads(template.read_text(encoding="utf-8").replace("@@INPUT_FILE@@", fixture.resolve().as_posix()))
pathlib.Path(output).write_text(json.dumps(spec), encoding="utf-8")
PY
    dc --as-tab "$CODETERM_TAB_ID" blueprint create --spec "$SPEC" --json
    ;;
  verify)
    [[ $# == 2 ]] || { echo "Usage: live.sh verify <blueprint_id>" >&2; exit 2; }
    id="$2"
    show="$(dc --as-tab "$CODETERM_TAB_ID" blueprint show "$id" --json)"
    log="$(dc --as-tab "$CODETERM_TAB_ID" blueprint log "$id" --limit 200 --json)"
    data_dir="$(dev_data_dir)"
    python3 - "$show" "$log" "$data_dir" "$CODETERM_TAB_ID" "$id" <<'PY'
import json, pathlib, sys
show, rows = json.loads(sys.argv[1]), json.loads(sys.argv[2])
data_dir, tab_id, blueprint_id = map(pathlib.Path, sys.argv[3:6])
steps = show.get("steps", [])
assert show.get("result", {}).get("state") == "delivered", show.get("result")
assert len(steps) == 1 and steps[0].get("kind") == "convert", steps
assert steps[0].get("state") == "done", steps
summary = steps[0].get("result_summary") or ""
assert "audio quick-brown-fox.wav -> text," in summary, summary
assert summary.endswith("chars"), summary
run_id = show.get("run_id")
assert run_id, "blueprint read model omitted its settled run id"
runs = {row.get("run_id") for row in rows if row.get("step_name") == "$run" and row.get("state") == "done"}
conversions = [row for row in rows if row.get("step_name") == "transcribe" and row.get("state") == "done"]
assert runs == {run_id}, f"expected one settled input activation, got {runs}"
assert len(conversions) == 1 and conversions[0].get("run_id") == run_id, conversions
run_dir = data_dir / "assets" / "blueprint-runs" / blueprint_id / run_id
assert not run_dir.exists(), f"input run directory remains: {run_dir}"
inbox = data_dir / "runtime" / "agent-pads" / tab_id / "inbox"
bodies = [path.read_text(encoding="utf-8") for path in inbox.glob("*") if path.is_file()]
matches = [body for body in bodies if "Input result (untrusted data)" in body and "quick brown fox" in body.lower()]
assert matches, f"owner inbox has no transcript body under {inbox}"
print(json.dumps({"blueprint": blueprint_id, "run_id": run_id, "summary": summary, "transcript_body": matches[-1]}, indent=2))
PY
    ;;
  *)
    echo "Usage: live.sh create | verify <blueprint_id>" >&2
    exit 2
    ;;
esac
