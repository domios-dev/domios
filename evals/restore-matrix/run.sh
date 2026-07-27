#!/usr/bin/env bash
set -u
set -o pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
source "$REPO_ROOT/evals/_lib/lib.sh"

export WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9223"
export VITE_CODETERM_TEST=1
export CODETERM_TEST_CLOCK=1

# Tunables (env-overridable so the matrix can be debugged one provider/tab at a
# time and capped in wall-time). Three tabs/provider proves same-provider
# multi-tab isolation while still keeping a full 4-provider run practical.
RM_PROVIDERS="${RM_PROVIDERS:-claude codex opencode cursor}"
RM_PER_PROVIDER="${RM_PER_PROVIDER:-3}"
RM_MATERIALIZE_TIMEOUT="${RM_MATERIALIZE_TIMEOUT:-60}"

RUN_ID="$(python - <<'PY'
import uuid
print(uuid.uuid4().hex[:10])
PY
)"
WORKSPACE_NAME="RESTORE-MATRIX-$RUN_ID"
WORKSPACE_CWD="C:/temp/$WORKSPACE_NAME"
RUN_DIR="$EVAL_LIB_OUT_BASE/restore-matrix-$RUN_ID"
MANIFEST="$RUN_DIR/manifest.json"
SLOTS_TSV="$RUN_DIR/slots.tsv"
SUCCESS=0
WORKSPACE_ID=""
mkdir -p "$RUN_DIR" "$WORKSPACE_CWD"
: > "$SLOTS_TSV"

cleanup() {
  if [ "$SUCCESS" = "1" ] && [ -n "$WORKSPACE_ID" ]; then
    echo "== cleanup workspace $WORKSPACE_NAME"
    dc workspace close "$WORKSPACE_ID" > /dev/null 2>&1 || true
  else
    echo "== keeping workspace for forensics: $WORKSPACE_NAME id=${WORKSPACE_ID:-unknown} manifest=$MANIFEST"
  fi
}
trap cleanup EXIT

write_manifest() {
  local phase="$1"
  PYTHONIOENCODING=utf-8 python - "$MANIFEST" "$phase" "$RUN_ID" "$WORKSPACE_NAME" "$WORKSPACE_ID" "$SLOTS_TSV" <<'PY'
import json, pathlib, sys
manifest, phase, run_id, ws_name, ws_id, tsv = sys.argv[1:7]
slots = []
for line in pathlib.Path(tsv).read_text(encoding='utf-8').splitlines():
    if not line.strip():
        continue
    parts = line.split('\t')
    while len(parts) < 11:
        parts.append('')
    provider, idx, model, tab_id, tab_id, marker, sid_before, sid_after, skipped, reason, booted_model = parts[:11]
    slots.append({
        'provider': provider,
        'index': int(idx),
        'requested_model': model,
        'booted_model_evidence': booted_model or None,
        'tab_id': tab_id or None,
        'tab_id': tab_id or None,
        'marker': marker,
        'sid_before': sid_before or None,
        'sid_after': sid_after or None,
        'skipped': skipped == '1',
        'skip_reason': reason or None,
    })
data = {
    'run_id': run_id,
    'phase': phase,
    'workspace': {'id': ws_id, 'name': ws_name},
    'slots': slots,
}
pathlib.Path(manifest).write_text(json.dumps(data, indent=2, sort_keys=True) + '\n', encoding='utf-8')
PY
}

replace_slot_field() {
  local provider="$1" idx="$2" field="$3" value="$4"
  local lock="$SLOTS_TSV.lock"
  while ! mkdir "$lock" 2>/dev/null; do
    sleep 0.1
  done
PYTHONIOENCODING=utf-8 python - "$SLOTS_TSV" "$provider" "$idx" "$field" "$value" <<'PY'
import os, pathlib, sys
path, provider, idx, field, value = sys.argv[1:6]
fields = ['provider','idx','model','tab_id','tab_id','marker','sid_before','sid_after','skipped','reason','booted_model']
col = fields.index(field)
rows = []
path = pathlib.Path(path)
for line in path.read_text(encoding='utf-8').splitlines():
    parts = line.split('\t')
    while len(parts) < len(fields):
        parts.append('')
    if parts[0] == provider and parts[1] == idx:
        parts[col] = value.replace('\t', ' ').replace('\n', ' ')
    rows.append('\t'.join(parts))
tmp = path.with_name(f"{path.name}.{os.getpid()}.tmp")
tmp.write_text('\n'.join(rows) + ('\n' if rows else ''), encoding='utf-8')
os.replace(tmp, path)
PY
  local rc=$?
  rmdir "$lock" 2>/dev/null || true
  return "$rc"
}

append_slot() {
  local provider="$1" idx="$2" model="$3" marker="$4"
  printf '%s\t%s\t%s\t\t\t%s\t\t\t0\t\t\n' "$provider" "$idx" "$model" "$marker" >> "$SLOTS_TSV"
}

json_field() {
  local expr="$1"
  python -c "import json,sys; raw=sys.stdin.read(); data=json.loads(raw) if raw.strip().startswith('{') else {}; print($expr or '')" 2>/dev/null
}

marker_in_text() {
  local marker="$1"
  PYTHONIOENCODING=utf-8 python - "$marker" <<'PY'
import re, sys
marker = sys.argv[1]
text = sys.stdin.read()
norm = re.sub(r"[^A-Za-z0-9]", "", text).lower()
needle = re.sub(r"[^A-Za-z0-9]", "", marker).lower()
sys.exit(0 if needle in norm else 1)
PY
}

marker_materialized_in_store() {
  local provider="$1" idx="$2"
  PYTHONIOENCODING=utf-8 python - "$SLOTS_TSV" "$provider" "$idx" "$WORKSPACE_NAME" "$WORKSPACE_CWD" <<'PY'
import json, os, pathlib, sqlite3, sys
slots_path, provider, idx, workspace_name = sys.argv[1:5]
workspace_cwd = sys.argv[5]
row = None
for line in pathlib.Path(slots_path).read_text(encoding="utf-8").splitlines():
    parts = line.split("\t")
    while len(parts) < 11:
        parts.append("")
    if parts[0] == provider and parts[1] == idx:
        row = parts
        break
if not row:
    sys.exit(1)
marker = row[5]
sid = row[6]
cwd = f"{workspace_cwd}/rm-{provider}-{idx}"
if not marker:
    sys.exit(1)
home = pathlib.Path.home()

def contains_marker(path: pathlib.Path) -> bool:
    try:
        return marker in path.read_text(encoding="utf-8", errors="ignore")
    except OSError:
        return False

if provider == "claude":
    projects = home / ".claude" / "projects"
    if not projects.exists():
        sys.exit(1)
    candidates = []
    if sid:
        candidates.extend(projects.glob(f"*{workspace_name}*rm-claude-{idx}/{sid}.jsonl"))
    candidates.extend(projects.glob(f"*{workspace_name}*rm-claude-{idx}/*.jsonl"))
    for path in candidates:
        if contains_marker(path):
            print(str(path))
            sys.exit(0)
    sys.exit(1)

if provider == "codex":
    root_env = os.environ.get("CODETERM_CODEX_SESSIONS_ROOT")
    root = pathlib.Path(root_env) if root_env else home / ".codex" / "sessions"
    if not root.exists():
        sys.exit(1)
    for path in root.rglob("*.jsonl"):
        if contains_marker(path):
            text = path.read_text(encoding="utf-8", errors="ignore")
            cwd_fwd = cwd.replace("\\", "/")
            cwd_bwd = cwd.replace("/", "\\")
            if workspace_name in text or cwd_fwd in text or cwd_bwd in text:
                print(str(path))
                sys.exit(0)
    sys.exit(1)

if provider == "opencode":
    data_home = os.environ.get("XDG_DATA_HOME")
    base = pathlib.Path(data_home) if data_home else home / ".local" / "share"
    db = base / "opencode" / "opencode.db"
    if db.exists():
        try:
            conn = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
            needles = [marker]
            for table in ("part", "message", "session"):
                try:
                    rows = conn.execute(f"SELECT id, data FROM {table} WHERE data LIKE ?", (f"%{marker}%",)).fetchall()
                except sqlite3.Error:
                    continue
                if rows:
                    print(f"{db}:{table}:{rows[0][0]}")
                    sys.exit(0)
        except sqlite3.Error:
            pass
    storage = base / "opencode" / "storage"
    if storage.exists():
        for path in storage.rglob("*.json"):
            if contains_marker(path):
                print(str(path))
                sys.exit(0)
    sys.exit(1)

if provider == "cursor":
    import re
    def cursor_slug(value: str) -> str:
        return re.sub(r"[^a-zA-Z0-9]", "-", value.lstrip("/\\"))
    project_roots = []
    for candidate_cwd in (cwd, str(pathlib.Path(cwd).resolve())):
        project = home / ".cursor" / "projects" / cursor_slug(candidate_cwd)
        if project not in project_roots:
            project_roots.append(project)
    candidates = []
    trusted = []
    for project in project_roots:
        trust = project / ".workspace-trusted"
        if not trust.exists():
            continue
        trusted.append(trust)
        if sid:
            candidates.append(project / "agent-transcripts" / sid / f"{sid}.jsonl")
        transcript_root = project / "agent-transcripts"
        if transcript_root.exists():
            candidates.extend(transcript_root.glob("*/*.jsonl"))
    if not trusted:
        sys.exit(1)
    seen = set()
    for path in candidates:
        key = str(path)
        if key in seen:
            continue
        seen.add(key)
        if contains_marker(path):
            print(f"{path}; trust={trusted[0]}")
            sys.exit(0)
    sys.exit(1)

sys.exit(1)
PY
}

detect_booted_model_evidence() {
  local provider="$1" tab_id="$2"
  dc tab get-text --tab "$tab_id" 2>/dev/null | PYTHONIOENCODING=utf-8 python - "$provider" <<'PY'
import re, sys
provider = sys.argv[1]
text = sys.stdin.read().lower()
evidence = "unknown"
if provider == "opencode":
    if "big-pickle" in text or "opencode/big-pickle" in text:
        evidence = "opencode/big-pickle"
    elif "mimo" in text:
        evidence = "mimo"
elif provider == "codex":
    m = re.search(r"gpt[-_. ]?5(?:[-_. ]?4)?(?:[-_. ]?mini)?", text)
    if m:
        evidence = m.group(0)
elif provider == "claude":
    for name in ("haiku", "sonnet", "opus", "fable"):
        if name in text:
            evidence = name
            break
elif provider == "cursor":
    for name in ("composer", "claude", "gpt", "gemini", "grok", "kimi", "auto"):
        if name in text:
            evidence = name
            break
print(evidence)
PY
}

tab_json_by_id() {
  local tab_id="$1"
  dev_get /api/tabs | python - "$tab_id" <<'PY'
import json, sys
tab_id = sys.argv[1]
for tab in json.load(sys.stdin):
    if tab.get('id') == tab_id:
        print(json.dumps(tab))
        sys.exit(0)
sys.exit(1)
PY
}

refresh_slot_pre() {
  local provider="$1" idx="$2" timeout="${3:-45}"
  local tab_id tab_id sid tab_json deadline
  tab_id=$(awk -F '\t' -v p="$provider" -v i="$idx" '$1==p && $2==i {print $4}' "$SLOTS_TSV")
  [ -n "$tab_id" ] || return 1
  deadline=$((SECONDS + timeout))
  while [ $SECONDS -lt $deadline ]; do
    tab_json=$(tab_json_by_id "$tab_id" 2>/dev/null || true)
    tab_id=$(echo "$tab_json" | json_field "data.get('tab_id')")
    sid=$(echo "$tab_json" | json_field "data.get('agent_session_id')")
    [ -n "$tab_id" ] && replace_slot_field "$provider" "$idx" tab_id "$tab_id"
    [ -n "$sid" ] && replace_slot_field "$provider" "$idx" sid_before "$sid"
    if [ -n "$tab_id" ] && [ -n "$sid" ]; then
      return 0
    fi
    sleep 2
  done
  return 0
}

refresh_all_slots_pre() {
  dev_get /api/tabs > "$RUN_DIR/pre-tabs.json"
  PYTHONIOENCODING=utf-8 python - "$SLOTS_TSV" "$RUN_DIR/pre-tabs.json" <<'PY'
import json, pathlib, sys
slots_path, tabs_path = sys.argv[1:3]
rows = [line.split('\t') for line in pathlib.Path(slots_path).read_text(encoding='utf-8').splitlines() if line.strip()]
for r in rows:
    while len(r) < 11:
        r.append('')
tabs = {p.get('id'): p for p in json.load(open(tabs_path, encoding='utf-8'))}
for r in rows:
    tab = tabs.get(r[3])
    if not tab:
        continue
    if tab.get('tab_id'):
        r[4] = tab.get('tab_id') or ''
    if tab.get('agent_session_id'):
        r[6] = tab.get('agent_session_id') or ''
pathlib.Path(slots_path).write_text('\n'.join('\t'.join(r) for r in rows) + '\n', encoding='utf-8')
PY
  local line line_usv provider idx model tab_id tab_id marker sid_before sid_after skipped reason booted_model
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    line_usv=${line//$'\t'/$'\034'}
    IFS=$'\034' read -r provider idx model tab_id tab_id marker sid_before sid_after skipped reason booted_model <<< "$line_usv"
    if [ -n "${tab_id:-}" ]; then
      replace_slot_field "$provider" "$idx" booted_model "$(detect_booted_model_evidence "$provider" "$tab_id")"
    fi
  done < "$SLOTS_TSV"
}

spawn_slot() {
  local provider="$1" idx="$2" model="$3" marker="$4"
  local title="rm-${provider}-${idx}"
  local cwd="$WORKSPACE_CWD/$title"
  mkdir -p "$cwd"
  local out tab_id tab_id sid
  out=$(dc agent spawn "$provider" --workspace "$WORKSPACE_ID" --cwd "$cwd" --title "$title" --model "$model" 2>&1)
  tab_id=$(echo "$out" | grep -oE 'tab [a-f0-9]+' | awk '{print $2}' | head -1)
  if [ -z "$tab_id" ]; then
    replace_slot_field "$provider" "$idx" skipped 1
    replace_slot_field "$provider" "$idx" reason "spawn failed: $out"
    echo "SKIP provider=$provider slot=$idx reason=spawn failed"
    return 1
  fi
  replace_slot_field "$provider" "$idx" tab_id "$tab_id"
  local tab_json
  tab_json=$(tab_json_by_id "$tab_id" 2>/dev/null || true)
  tab_id=$(echo "$tab_json" | json_field "data.get('tab_id')")
  sid=$(echo "$tab_json" | json_field "data.get('agent_session_id')")
  replace_slot_field "$provider" "$idx" tab_id "$tab_id"
  replace_slot_field "$provider" "$idx" sid_before "$sid"
  replace_slot_field "$provider" "$idx" booted_model "$(detect_booted_model_evidence "$provider" "$tab_id")"
  echo "SPAWN provider=$provider slot=$idx tab=$tab_id sid=${sid:-none} marker=$marker"
  return 0
}

send_marker_and_wait() {
  local provider="$1" idx="$2" tab_id="$3" marker="$4"
  local prompt="Reply with exactly $marker and nothing else."
  local out deadline payload response status body attempt evidence

  payload=$(python - "$prompt" <<'PY'
import json, sys
print(json.dumps({"text": sys.argv[1], "enter": True}))
PY
)
  for attempt in 1 2; do
    response=$(curl -s --max-time 120 -w '\n%{http_code}' -X POST -H "$CODETERM_DEV_AUTH_HEADER" -H "Content-Type: application/json" --data "$payload" "http://127.0.0.1:7686/api/tabs/$tab_id/send")
    status=$(printf '%s' "$response" | tail -n 1)
    body=$(printf '%s' "$response" | sed '$d')
    if [ "$status" != "200" ]; then
      replace_slot_field "$provider" "$idx" skipped 1
      replace_slot_field "$provider" "$idx" reason "send failed HTTP $status: $body"
      echo "SKIP provider=$provider slot=$idx tab=$tab_id reason=send failed HTTP $status: $body"
      return 1
    fi
    deadline=$((SECONDS + RM_MATERIALIZE_TIMEOUT))
    while [ $SECONDS -lt $deadline ]; do
      refresh_slot_pre "$provider" "$idx" 1 || true
      evidence=$(marker_materialized_in_store "$provider" "$idx" 2>/dev/null || true)
      if [ -n "$evidence" ]; then
        echo "PASS pre-marker-store provider=$provider slot=$idx tab=$tab_id marker=$marker evidence=$evidence"
        return 0
      fi
      out=$(dc tab get-text --tab "$tab_id" 2>&1 || true)
      if printf '%s' "$out" | marker_in_text "$marker"; then
        echo "INFO pre-marker-terminal-visible provider=$provider slot=$idx tab=$tab_id marker=$marker waiting_for_provider_store"
      fi
      if echo "$out" | grep -Eiq 'rate-limit-options|session limit|usage limit|upgrade your plan|sign in|login|authenticate'; then
        replace_slot_field "$provider" "$idx" skipped 1
        replace_slot_field "$provider" "$idx" reason "auth/rate-limit screen before marker materialized"
        echo "SKIP provider=$provider slot=$idx tab=$tab_id reason=auth/rate-limit screen before marker materialized"
        return 1
      fi
      sleep 5
    done
    echo "WARN provider=$provider slot=$idx tab=$tab_id marker did not materialize after send attempt $attempt"
  done
  replace_slot_field "$provider" "$idx" skipped 1
  replace_slot_field "$provider" "$idx" reason "send/setup failure: marker did not materialize pre-kill after retry"
  echo "SEND_SETUP_FAIL provider=$provider slot=$idx tab=$tab_id reason=marker did not materialize pre-kill after retry"
  return 1
}

provider_model() {
  case "$1" in
    claude) echo haiku ;;
    codex) echo gpt-5.4-mini ;;
    cursor) echo auto ;;
    opencode)
      local providers_json
      providers_json="$(dev_get /api/agents/providers 2>/dev/null || true)"
      python -c '
import json, sys
try:
    providers = json.loads(sys.stdin.read() or "[]")
except Exception:
    providers = []
target = next((p for p in providers if p.get("id") == "opencode"), {})
ids = [m.get("id") for m in target.get("available_models", []) if m.get("id")]
default = target.get("default_model")
if "opencode/big-pickle" in ids:
    print("opencode/big-pickle")
elif default:
    print(default)
elif ids:
    print(ids[0])
else:
    print("opencode/big-pickle")
' <<< "$providers_json"
      ;;
    *) return 1 ;;
  esac
}

new_marker() {
  python - <<'PY'
import uuid
print("RESTORE-MARKER-" + str(uuid.uuid4()))
PY
}

prepare_slots() {
  local provider idx model marker
  for provider in $RM_PROVIDERS; do
    if ! model="$(provider_model "$provider")"; then
      model="unknown"
    fi
    for idx in $(seq 1 "$RM_PER_PROVIDER"); do
      marker="$(new_marker)"
      append_slot "$provider" "$idx" "$model" "$marker"
    done
  done
}

spawn_all_slots() {
  local pids=() line line_usv provider idx model marker _tab _tab _sid_before _sid_after _skipped _reason _booted_model
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    line_usv=${line//$'\t'/$'\034'}
    IFS=$'\034' read -r provider idx model _tab _tab marker _sid_before _sid_after _skipped _reason _booted_model <<< "$line_usv"
    (
      spawn_slot "$provider" "$idx" "$model" "$marker"
    ) &
    pids+=("$!")
  done < "$SLOTS_TSV"
  for pid in "${pids[@]}"; do
    wait "$pid" || true
  done
}

send_all_markers() {
  local pids=() line line_usv provider idx model tab_id tab_id marker sid_before sid_after skipped reason booted_model
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    line_usv=${line//$'\t'/$'\034'}
    IFS=$'\034' read -r provider idx model tab_id tab_id marker sid_before sid_after skipped reason booted_model <<< "$line_usv"
    if [ -n "${tab_id:-}" ] && [ "${skipped:-0}" != "1" ]; then
      (
        send_marker_and_wait "$provider" "$idx" "$tab_id" "$marker"
      ) &
      pids+=("$!")
    fi
  done < "$SLOTS_TSV"
  for pid in "${pids[@]}"; do
    wait "$pid" || true
  done
}

assert_post_chat_isolation() {
  PYTHONIOENCODING=utf-8 python - "$SLOTS_TSV" <<'PY'
import json, os, pathlib, re, sys, urllib.error, urllib.request
slots_path = pathlib.Path(sys.argv[1])
token = os.environ.get("CODETERM_API_TOKEN", "")
fields = ['provider','idx','model','tab_id','tab_id','marker','sid_before','sid_after','skipped','reason','booted_model']
rows = []
for line in slots_path.read_text(encoding="utf-8").splitlines():
    if not line.strip():
        continue
    parts = line.split("\t")
    while len(parts) < len(fields):
        parts.append("")
    rows.append(dict(zip(fields, parts)))

active = [r for r in rows if r["skipped"] != "1"]
markers = [r["marker"] for r in active]

def clean(text):
    return re.sub(r"[^A-Za-z0-9]", "", text).lower()

def api_get(path):
    req = urllib.request.Request(
        "http://127.0.0.1:7686" + path,
        headers={"Authorization": "Bearer " + token},
    )
    with urllib.request.urlopen(req, timeout=30) as response:
        return json.loads(response.read().decode("utf-8"))

for row in active:
    tab_id = row["tab_id"]
    marker = row["marker"]
    if not tab_id:
        raise SystemExit(f"FAIL chat provider={row['provider']} slot={row['idx']}: missing tab_id")
    try:
        payload = api_get(f"/api/tabs/{tab_id}/chat")
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")
        raise SystemExit(f"FAIL chat provider={row['provider']} slot={row['idx']} tab={tab_id}: HTTP {exc.code}: {body[:300]}")
    text = json.dumps(payload.get("messages", []), ensure_ascii=False)
    norm = clean(text)
    own = clean(marker)
    if own not in norm:
        raise SystemExit(f"FAIL chat provider={row['provider']} slot={row['idx']} tab={tab_id}: marker missing from /chat marker={marker}")
    for other in markers:
        if other == marker:
            continue
        if clean(other) in norm:
            raise SystemExit(f"FAIL chat provider={row['provider']} slot={row['idx']} tab={tab_id}: /chat contains other marker {other}")
    print(f"PASS chat provider={row['provider']} slot={row['idx']} tab={tab_id} marker={marker}")
PY
}

refresh_post_sids_and_assert_api() {
  local tabs_json="$RUN_DIR/post-tabs.json"
  dev_get /api/tabs > "$tabs_json"
  PYTHONIOENCODING=utf-8 python - "$SLOTS_TSV" "$tabs_json" <<'PY'
import pathlib, sys, json
slots_path, tabs_path = sys.argv[1:3]
rows = [line.split('\t') for line in pathlib.Path(slots_path).read_text(encoding='utf-8').splitlines() if line.strip()]
tabs = {p.get('id'): p for p in json.load(open(tabs_path, encoding='utf-8'))}
pre_sid_owner = {r[6]: r[3] for r in rows if r[6] and r[8] != '1'}
post_sid_owner = {}
def assertable(r):
    marker, skipped, reason = r[5], r[8], r[9]
    if skipped != '1':
        return True
    # If we successfully created a tab and sent a unique marker but timed out
    # waiting for it, that tab is still an oracle target. The UI must not be
    # allowed to hide a fresh/wrong restore behind a provider-level skip.
    return bool(r[3]) and marker.startswith('RESTORE-MARKER-') and not marker.startswith('RESTORE-MARKER-skipped-') and reason == 'marker did not appear before timeout'

for r in rows:
    while len(r) < 11:
        r.append('')
    provider, idx, model, tab_id, tab_id, marker, sid_before, sid_after, skipped, reason, booted_model = r
    if not tab_id:
        continue
    tab = tabs.get(tab_id)
    if not tab:
        if assertable(r):
            raise SystemExit(f"FAIL provider={provider} slot={idx} tab={tab_id}: tab missing after restart")
        continue
    if tab.get('tab_id') and not tab_id:
        r[4] = tab.get('tab_id') or ''
    sid_after = tab.get('agent_session_id') or ''
    r[7] = sid_after
    if not assertable(r):
        print(f"SKIP api provider={provider} slot={idx} tab={tab_id} reason={reason or 'provider skipped'} sid_before={sid_before or 'none'} sid_after={sid_after or 'none'}")
        continue
    if tab.get('agent_type') != provider:
        raise SystemExit(f"FAIL provider={provider} slot={idx} tab={tab_id}: provider changed to {tab.get('agent_type')!r}")
    if sid_before and sid_after and sid_after != sid_before:
        raise SystemExit(f"FAIL provider={provider} slot={idx} tab={tab_id}: sid drift {sid_before} -> {sid_after}")
    if sid_before and not sid_after:
        raise SystemExit(f"FAIL provider={provider} slot={idx} tab={tab_id}: sid missing after restart; before={sid_before}")
    if not sid_before and not sid_after:
        print(f"SKIP sid-check provider={provider} slot={idx} tab={tab_id} reason=unsupported-or-not-detected")
    if sid_after:
        other = pre_sid_owner.get(sid_after)
        if other and other != tab_id:
            raise SystemExit(f"FAIL provider={provider} slot={idx} tab={tab_id}: holds another tab sid {sid_after} owner={other}")
        if sid_after in post_sid_owner and post_sid_owner[sid_after] != tab_id:
            raise SystemExit(f"FAIL provider={provider} slot={idx} tab={tab_id}: duplicate post sid {sid_after} also={post_sid_owner[sid_after]}")
        post_sid_owner[sid_after] = tab_id
    print(f"PASS api provider={provider} slot={idx} tab={tab_id} sid_before={sid_before or 'none'} sid_after={sid_after or 'none'}")
pathlib.Path(slots_path).write_text('\n'.join('\t'.join(r) for r in rows) + '\n', encoding='utf-8')
PY
}

print_slot_table() {
  awk -F '\t' 'BEGIN {
      printf "provider\tslot\ttab\trequested_model\tbooted_model\tsid_before\tsid_after\tstatus\treason\tmarker\n"
    }
    {
      status=($9=="1" ? "SKIP" : "PASS");
      printf "%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n", $1,$2,($4?$4:"-"),$3,($11?$11:"unknown"),($7?$7:"none"),($8?$8:"none"),status,($10?$10:"-"),$6
    }' "$SLOTS_TSV"
}

echo "== reset dev to clean restore-matrix baseline"
reset_dev_state_hard || exit 1

echo "== create matrix workspace $WORKSPACE_NAME"
WORKSPACE_ID=$(new_workspace "$WORKSPACE_NAME" "$WORKSPACE_CWD")
[ -n "$WORKSPACE_ID" ] || { echo "FAIL could not create workspace"; exit 1; }

echo "== prepare slots providers=[$RM_PROVIDERS] per_provider=$RM_PER_PROVIDER"
prepare_slots

echo "== parallel spawn all slots"
spawn_all_slots

echo "== parallel marker sends and pre-kill materialization gates"
send_all_markers

echo "== refresh pre-restart tab/session/model evidence"
refresh_all_slots_pre || exit 1

active_count=$(awk -F '\t' '$9 != "1" {count++} END {print count+0}' "$SLOTS_TSV")
if [ "$active_count" -eq 0 ]; then
  write_manifest all-skipped
  echo "FAIL all providers skipped or failed pre-kill materialization; nothing restore-tested"
  print_slot_table
  exit 1
fi

write_manifest pre-restart

echo "== save session before hard restart"
dev_post /api/session/save > /dev/null || true
sleep 3

echo "== hard restart dev with Playwright/CDP env"
(
  cd "$REPO_ROOT" || exit 1
  bash build.sh kill
  WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="$WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS" VITE_CODETERM_TEST=1 CODETERM_TEST_CLOCK=1 bash build.sh dev --wait
) || exit 1
sleep 5

echo "== API post-restore assertions"
refresh_post_sids_and_assert_api || exit 1
write_manifest post-api

echo "== post-restore /chat isolation assertions"
assert_post_chat_isolation || exit 1

echo "== Playwright UI-visible marker oracle"
if ! (
  cd "$REPO_ROOT" || exit 1
  RESTORE_MATRIX_MANIFEST="$MANIFEST" pnpm exec playwright test -c tests/playwright --project=full tests/playwright/suites/10-restore/restore-matrix.spec.ts
); then
  echo "FAIL Playwright UI-visible marker oracle manifest=$MANIFEST"
  print_slot_table
  exit 1
fi

echo "== restore matrix PASS manifest=$MANIFEST"
print_slot_table
awk -F '\t' '$9 != "1" {printf "PASS provider=%s slot=%s tab=%s requested_model=%s booted_model=%s marker=%s sid_before=%s sid_after=%s\n", $1,$2,$4,$3,($11?$11:"unknown"),$6,($7?$7:"none"),($8?$8:"none")}' "$SLOTS_TSV"
awk -F '\t' '$9 == "1" {printf "SKIP provider=%s slot=%s requested_model=%s booted_model=%s reason=%s\n", $1,$2,$3,($11?$11:"unknown"),$10}' "$SLOTS_TSV"
SUCCESS=1
