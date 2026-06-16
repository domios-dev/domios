# CLI Agent Benchmark — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A real-daemon integration benchmark that drives real agents (claude/codex/opencode) in the dev console, exercises every CLI command via natural-language asks, and lets the model judge the parsed chat transcripts — surfacing and fixing doc/skill/CLI bugs.

**Architecture:** A new stateless `pane chat` read (REST + CLI) returns normalized chat messages. A bash harness under `evals/cli-agent/` (on top of `evals/_lib/lib.sh`) spawns clean panes, sends prompts, polls the read, and dumps transcripts. Declarative `cases/*.yaml` define asks + safety; the model judges transcripts on a 5-dimension rubric and fixes bugs in-loop.

**Tech Stack:** Rust (axum REST, clap CLI, serde), bash + curl + jq + python3 (yaml parse), the existing dev-locked `dc`/`dev_get` helpers.

Spec: `docs/superpowers/specs/2026-06-08-cli-agent-benchmark-design.md`

---

## File Structure

**Rust (read primitive):**
- Modify `src-tauri/src/service/pane_chat/mod.rs` — `pane_chat_snapshot()` service fn + `rich_to_chat_msgs()` converter + test.
- Modify `src-tauri/src/api/pane_chat_routes.rs` — `GET /api/panes/:id/chat` handler.
- Modify `src-tauri/src/api/routes.rs` (or wherever routes mount) — register the GET route.
- Modify `cli/src/args/misc.rs` — `PaneAction::Chat` variant.
- Modify `cli/src/cmd/pane.rs` — handle `Chat` via local HTTP (Config pattern).
- Regenerate `docs/cli-manifest.json` + `docs/CLI.md`.

**Harness (`evals/cli-agent/`):**
- Create `lib.sh`, `run-case.sh`, `run-suite.sh`, `coverage-check.sh`.
- Create `cases/workspace.yaml`, `cases/mem.yaml` (first batch), then expand.
- Create `judge-template.md`, `SKILL.md`, `README.md`, `.gitignore`.

**Cleanup:**
- Delete `evals/general-agent/`, `evals/mem-protocol/`.
- Modify `evals/README.md`.

---

## PART A — Read primitive (Rust)

### Task 1: `pane_chat_snapshot` service fn + converter

**Files:**
- Modify `src-tauri/src/service/pane_chat/mod.rs`

- [ ] **Step 1: Add the converter + snapshot fn with a unit test**

Append to `src-tauri/src/service/pane_chat/mod.rs`:

```rust
use crate::agent::chat_types::{ChatMsg, RichMessage};

/// Flatten a JSONL RichMessage (role + typed blocks) into the same flat
/// ChatMsg shape opencode emits, so the read endpoint is provider-uniform.
pub fn rich_to_chat_msgs(rich: &RichMessage, base_id: usize) -> Vec<ChatMsg> {
    let mut out = Vec::new();
    for (i, b) in rich.blocks.iter().enumerate() {
        let id = format!("{base_id}-{i}");
        let get = |k: &str| b.data.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let msg = match b.kind.as_str() {
            "text" => ChatMsg { id, kind: rich.role.clone(), content: get("text"), ..Default::default() },
            "thinking" => ChatMsg { id, kind: "thinking".into(), content: get("text"), ..Default::default() },
            "tool_use" => ChatMsg {
                id,
                kind: "tool_call".into(),
                content: String::new(),
                tool_name: b.data.get("name").and_then(|v| v.as_str()).map(String::from),
                tool_input: b.data.get("input").cloned(),
                ..Default::default()
            },
            "tool_result" => ChatMsg {
                id,
                kind: "tool_result".into(),
                content: String::new(),
                tool_result: Some(get("content")),
                ..Default::default()
            },
            _ => continue,
        };
        out.push(msg);
    }
    out
}

/// Stateless normalized-chat read for a pane: parses the whole session
/// (JSONL or sqlite) into flat ChatMsg list, returns messages[since..].
/// No tailer spawned. Returns (messages, total_count).
pub fn pane_chat_snapshot(
    svc: &crate::service::Service,
    pane_id: &str,
    since: usize,
) -> Result<(Vec<ChatMsg>, usize), String> {
    let (provider_id, cwd, session_id) = {
        let m = svc.mux.lock();
        let pane = m.get_pane(pane_id).ok_or("pane not found")?;
        let provider = pane.agent_type.clone().ok_or("pane has no agent_type")?;
        let cwd = pane.cwd.clone().unwrap_or_default();
        let sid = pane.agent_session_id.clone().ok_or("pane has no session_id yet")?;
        (provider, cwd, sid)
    };

    let structured = svc
        .agent_registry
        .with_provider(&provider_id, |p| p.uses_structured_chat())
        .unwrap_or(false);

    let all: Vec<ChatMsg> = if structured {
        svc.agent_registry
            .with_provider(&provider_id, |p| p.read_structured_chat(&cwd, &session_id, None))
            .map(|sc| sc.messages)
            .unwrap_or_default()
    } else {
        let path = svc
            .agent_registry
            .with_provider(&provider_id, |p| p.session_jsonl_path(&cwd, &session_id))
            .flatten()
            .ok_or("provider has no JSONL and no structured chat")?;
        let delta = svc
            .agent_registry
            .with_provider(&provider_id, |p| p.parse_session_delta(&path, 0))
            .ok_or("provider missing")?;
        delta
            .messages
            .iter()
            .enumerate()
            .flat_map(|(i, r)| rich_to_chat_msgs(r, i))
            .collect()
    };

    let total = all.len();
    let slice = all.into_iter().skip(since).collect();
    Ok((slice, total))
}

#[cfg(test)]
mod chat_snapshot_tests {
    use super::*;
    use crate::agent::chat_types::{MessageBlock, RichMessage};
    use serde_json::json;

    #[test]
    fn rich_text_and_tool_flatten() {
        let rich = RichMessage {
            role: "assistant".into(),
            ts: None,
            blocks: vec![
                MessageBlock { kind: "text".into(), data: json!({"text": "hi"}) },
                MessageBlock { kind: "tool_use".into(), data: json!({"name": "Bash", "input": {"command": "codeterm pane list"}}) },
            ],
        };
        let msgs = rich_to_chat_msgs(&rich, 0);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].kind, "assistant");
        assert_eq!(msgs[0].content, "hi");
        assert_eq!(msgs[1].kind, "tool_call");
        assert_eq!(msgs[1].tool_name.as_deref(), Some("Bash"));
        assert_eq!(msgs[1].tool_input.as_ref().unwrap()["command"], "codeterm pane list");
    }
}
```

- [ ] **Step 2: Confirm `Service` field/method names compile**

Run: `cd src-tauri && cargo test service::pane_chat::chat_snapshot_tests`
Expected: PASS. If `svc.mux` / `svc.agent_registry` / `with_provider` names differ, grep `src-tauri/src/api/pane_chat_routes.rs` (it uses `state.svc.mux`, `state.svc.agent_registry.with_provider`) and match them.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/service/pane_chat/mod.rs
git commit -m "feat(chat): stateless pane_chat_snapshot + RichMessage→ChatMsg converter"
```

---

### Task 2: `GET /api/panes/:id/chat` endpoint

**Files:**
- Modify `src-tauri/src/api/pane_chat_routes.rs`
- Modify the router (find where `chat/start` is mounted — grep `chat/start` in `src-tauri/src/api/`)

- [ ] **Step 1: Add the GET handler**

Append to `src-tauri/src/api/pane_chat_routes.rs`:

```rust
use axum::extract::Query;

#[derive(Deserialize)]
pub struct ChatQuery {
    #[serde(default)]
    pub since: usize,
}

/// GET /api/panes/:id/chat?since=N — stateless normalized-chat read.
pub async fn snapshot(
    AxumState(state): AxumState<AppState>,
    Path(pane_id): Path<String>,
    Query(q): Query<ChatQuery>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let (messages, total) =
        crate::service::pane_chat::pane_chat_snapshot(&state.svc, &pane_id, q.since)
            .map_err(|_| (StatusCode::BAD_REQUEST, "chat snapshot failed"))?;
    Ok(Json(serde_json::json!({ "messages": messages, "count": total })))
}
```

- [ ] **Step 2: Register the route**

Find where `/api/panes/:id/chat/start` is registered (grep `chat/start` under `src-tauri/src/api/`). Next to it add:

```rust
        .route("/api/panes/:id/chat", axum::routing::get(pane_chat_routes::snapshot))
```

- [ ] **Step 3: Build + manual smoke**

Run: `cd src-tauri && cargo build`
Expected: clean.

After dev is up with a chat pane: `curl -s -H "$DEV_AUTH" "http://127.0.0.1:7686/api/panes/<id>/chat?since=0" | jq '.count'` returns a number.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/api/pane_chat_routes.rs src-tauri/src/api/routes.rs
git commit -m "feat(api): GET /api/panes/:id/chat stateless normalized read"
```

---

### Task 3: `codeterm pane chat` CLI command

**Files:**
- Modify `cli/src/args/misc.rs` (PaneAction enum)
- Modify `cli/src/cmd/pane.rs`
- Regenerate `docs/cli-manifest.json` + `docs/CLI.md`

- [ ] **Step 1: Add the clap variant**

In `cli/src/args/misc.rs`, inside `enum PaneAction`, add:

```rust
    /// Read a pane's parsed chat transcript (text/thinking/tool calls/results)
    Chat {
        /// Target pane ID (default: $CODETERM_PANE_ID or focused pane)
        #[arg(long, alias = "id")]
        pane: Option<String>,
        /// Only return messages from this index onward
        #[arg(long, default_value = "0")]
        since: usize,
    },
```

- [ ] **Step 2: Handle it (local HTTP, mirrors Config)**

In `cli/src/cmd/pane.rs`, find the `match action` and add a `PaneAction::Chat` arm. Use the same local-HTTP access the `Config` command uses (`read_api_config()` → base_url + token). Reference `cli/src/cmd/agent.rs` for the `http`/`base` setup pattern. Add:

```rust
        PaneAction::Chat { pane, since } => {
            let pane_id = resolve_pane(client, pane);
            let (base, token) = crate::read_api_config();
            let http = reqwest::blocking::Client::new();
            let url = format!("{}/api/panes/{}/chat?since={}", base, pane_id, since);
            let resp = http.get(&url).bearer_auth(token).send();
            match resp.and_then(|r| r.json::<serde_json::Value>()) {
                Ok(v) => handle_result(json, Ok(v)),
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
```

(Confirm `read_api_config` visibility — it's used by the Config command; if private to `main.rs`, expose it `pub(crate)` or replicate its few lines. Confirm `handle_pane` already receives `client` + `json`.)

- [ ] **Step 3: Build + regenerate the SoT artifacts**

Run: `cd cli && cargo build && cargo run -q -- cli-manifest --write`
Expected: builds; `docs/cli-manifest.json` + `docs/CLI.md` now include `pane chat`.

- [ ] **Step 4: SoT tests stay green**

Run: `cd cli && cargo test cmd::manifest::tests`
Expected: PASS (fresh manifest + doctor).

- [ ] **Step 5: Commit**

```bash
git add cli/src/args/misc.rs cli/src/cmd/pane.rs docs/cli-manifest.json docs/CLI.md
git commit -m "feat(cli): pane chat — read normalized transcript"
```

---

## PART B — Harness

### Task 4: `evals/cli-agent/lib.sh` + `run-case.sh`

**Files:**
- Create `evals/cli-agent/lib.sh`
- Create `evals/cli-agent/run-case.sh`
- Create `evals/cli-agent/.gitignore`

- [ ] **Step 1: `lib.sh` — harness helpers on top of `_lib/lib.sh`**

Create `evals/cli-agent/lib.sh`:

```bash
#!/usr/bin/env bash
# CLI-agent benchmark helpers. Sources the dev-locked _lib/lib.sh (dc, dev_get).
set -euo pipefail
HARNESS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$HARNESS_DIR/../.." && pwd)"
source "$REPO_ROOT/evals/_lib/lib.sh"   # auto-calls dev_env (port 7686 + token)

# Read a pane's parsed transcript as JSON: messages[].{type,content,toolName,toolInput,toolResult}
read_chat() {  # <pane_id> [since]
  dev_get "/api/panes/$1/chat?since=${2:-0}"
}

# Wait until the transcript stops growing for `quiet` seconds, or `timeout` total.
wait_idle() {  # <pane_id> <timeout_s> <quiet_s>
  local pane="$1" timeout="$2" quiet="$3" start last_count=-1 last_change
  start=$(date +%s); last_change=$start
  while :; do
    local now count
    now=$(date +%s)
    count=$(read_chat "$pane" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("count",0))' 2>/dev/null || echo 0)
    if [ "$count" != "$last_count" ]; then last_count=$count; last_change=$now; fi
    [ $((now - last_change)) -ge "$quiet" ] && return 0
    [ $((now - start)) -ge "$timeout" ] && return 0
    sleep 2
  done
}
```

(`date`/`sleep` here run in a foreground bench script, not the agent loop — fine.)

- [ ] **Step 2: `run-case.sh` — drive one case for one provider**

Create `evals/cli-agent/run-case.sh`:

```bash
#!/usr/bin/env bash
# Usage: run-case.sh <case-file.yaml> <case-id> <provider> <runs-dir>
set -euo pipefail
HARNESS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$HARNESS_DIR/lib.sh"

CASE_FILE="$1"; CASE_ID="$2"; PROVIDER="$3"; RUNS_DIR="$4"
mkdir -p "$RUNS_DIR"

# Extract the case fields with python3 (no yq dependency).
read_field() {  # <field>
  python3 - "$CASE_FILE" "$CASE_ID" "$1" <<'PY'
import sys, yaml
cases = yaml.safe_load(open(sys.argv[1])) or []
c = next((x for x in cases if x.get("id") == sys.argv[2]), None)
print("" if c is None else (c.get(sys.argv[3], "") or ""))
PY
}

PROMPT="$(read_field prompt)"
SAFETY="$(read_field safety)"
[ -z "$PROMPT" ] && { echo "no prompt for $CASE_ID"; exit 2; }

TMP="$(mktemp -d)"
WS_JSON="$(dc workspace create "bench-$CASE_ID-$PROVIDER" --cwd "$TMP" --json)"
WS="$(echo "$WS_JSON" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("id",""))')"
SPAWN="$(dc agent spawn "$PROVIDER" --workspace "$WS" --cwd "$TMP" --title "$CASE_ID" --json)"
PANE="$(echo "$SPAWN" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("pane_id",""))')"

if [ -z "$PANE" ]; then
  echo "{\"case\":\"$CASE_ID\",\"provider\":\"$PROVIDER\",\"status\":\"SKIPPED\",\"reason\":\"spawn failed\"}" \
    > "$RUNS_DIR/$CASE_ID-$PROVIDER.json"
  dc workspace close "$WS" >/dev/null 2>&1 || true
  exit 0
fi

sleep 4
dc send "$PROMPT" --pane "$PANE" >/dev/null
wait_idle "$PANE" 120 12

read_chat "$PANE" > "$RUNS_DIR/$CASE_ID-$PROVIDER.json"
# Human-readable transcript for the judge.
python3 "$HARNESS_DIR/transcript-md.py" "$RUNS_DIR/$CASE_ID-$PROVIDER.json" \
  "$CASE_ID" "$PROVIDER" "$PROMPT" > "$RUNS_DIR/$CASE_ID-$PROVIDER.md"

dc workspace close "$WS" >/dev/null 2>&1 || true
rm -rf "$TMP"
echo "wrote $RUNS_DIR/$CASE_ID-$PROVIDER.md"
```

- [ ] **Step 3: `transcript-md.py` — render JSON transcript to readable markdown**

Create `evals/cli-agent/transcript-md.py`:

```python
import sys, json
data = json.load(open(sys.argv[1]))
case, provider, prompt = sys.argv[2], sys.argv[3], sys.argv[4]
print(f"# {case} — {provider}\n\n**Prompt:** {prompt}\n")
for m in data.get("messages", []):
    t = m.get("type", "?")
    if t == "tool_call":
        cmd = (m.get("toolInput") or {}).get("command", "")
        print(f"- 🔧 **{m.get('toolName','tool')}** `{cmd}`")
    elif t == "tool_result":
        print(f"  ↳ result: {(m.get('toolResult') or '')[:300]}")
    elif t == "thinking":
        print(f"- 💭 _{m.get('content','')[:200]}_")
    else:
        print(f"- **{t}:** {m.get('content','')[:500]}")
```

- [ ] **Step 4: `.gitignore` — keep transcripts out of git except samples**

Create `evals/cli-agent/.gitignore`:

```
runs/
!runs/sample/
```

- [ ] **Step 5: chmod + commit**

```bash
chmod +x evals/cli-agent/run-case.sh evals/cli-agent/lib.sh
git add evals/cli-agent/lib.sh evals/cli-agent/run-case.sh evals/cli-agent/transcript-md.py evals/cli-agent/.gitignore
git commit -m "feat(evals): cli-agent harness — drive one case, dump transcript"
```

---

### Task 5: Case format + first batch + coverage check

**Files:**
- Create `evals/cli-agent/cases/workspace.yaml`, `evals/cli-agent/cases/mem.yaml`
- Create `evals/cli-agent/coverage-check.sh`

- [ ] **Step 1: First case files**

Create `evals/cli-agent/cases/mem.yaml`:

```yaml
- id: mem.search.basic
  command: "mem search"
  prompt: "Search our memory for anything about the Telegram bridge and summarize what you find."
  expect: |
    Runs `codeterm mem search "<telegram bridge>"`, reads results, summarizes.
    Must not invent `mem find`; must not fail on socket/port.
  safety: execute
- id: mem.save.basic
  command: "mem save"
  prompt: "Remember this fact: the CLI-agent benchmark drives real dev-console agents, not headless -p."
  expect: |
    Runs `codeterm mem save --kind <fact|note> --title "..." "<body>"` in English.
  safety: execute
- id: mem.get.basic
  command: "mem get"
  prompt: "Search memory for the benchmark fact you just might find, then print the full body of one matching item by id."
  expect: |
    `mem search` then `mem get <real-id>` — must use a real id from search, not guess.
  safety: execute
```

Create `evals/cli-agent/cases/workspace.yaml`:

```yaml
- id: workspace.create.basic
  command: "workspace create"
  prompt: "Create a new workspace called 'scratch-bench'."
  expect: "Runs `codeterm workspace create \"scratch-bench\"`."
  safety: execute
- id: workspace.list.basic
  command: "workspace list"
  prompt: "List all open workspaces and tell me how many there are."
  expect: "Runs `codeterm workspace list`, counts them."
  safety: execute
```

- [ ] **Step 2: `coverage-check.sh` — every manifest command has a case**

Create `evals/cli-agent/coverage-check.sh`:

```bash
#!/usr/bin/env bash
# Report CLI commands (from docs/cli-manifest.json) with no benchmark case.
set -euo pipefail
HARNESS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$HARNESS_DIR/../.." && pwd)"
python3 - "$REPO_ROOT/docs/cli-manifest.json" "$HARNESS_DIR/cases" <<'PY'
import sys, json, glob, os, yaml
manifest = json.load(open(sys.argv[1]))
def paths(node, prefix=()):
    for c in node.get("subcommands", []):
        p = prefix + (c["name"],)
        if c.get("subcommands"):
            yield from paths(c, p)
        else:
            yield " ".join(p)
all_cmds = set(paths(manifest))
covered = set()
for f in glob.glob(os.path.join(sys.argv[2], "*.yaml")):
    for case in (yaml.safe_load(open(f)) or []):
        covered.add(case.get("command", ""))
missing = sorted(all_cmds - covered)
print(f"{len(covered & all_cmds)}/{len(all_cmds)} leaf commands covered")
if missing:
    print("UNCOVERED:")
    for m in missing: print(f"  {m}")
PY
```

- [ ] **Step 3: Run coverage (expected: many uncovered initially)**

Run: `bash evals/cli-agent/coverage-check.sh`
Expected: prints a count + a long UNCOVERED list (Part D fills it in).

- [ ] **Step 4: Commit**

```bash
chmod +x evals/cli-agent/coverage-check.sh
git add evals/cli-agent/cases evals/cli-agent/coverage-check.sh
git commit -m "feat(evals): cli-agent case format + first batch + coverage check"
```

---

### Task 6: `run-suite.sh` — cases × providers in parallel

**Files:**
- Create `evals/cli-agent/run-suite.sh`

- [ ] **Step 1: Suite runner**

Create `evals/cli-agent/run-suite.sh`:

```bash
#!/usr/bin/env bash
# Usage: run-suite.sh [provider1,provider2,...] [case-glob]
# Runs each case across providers; providers parallel per case, cases sequential.
set -euo pipefail
HARNESS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$HARNESS_DIR/lib.sh"

PROVIDERS="${1:-claude,codex,opencode}"
GLOB="${2:-$HARNESS_DIR/cases/*.yaml}"
RUNS_DIR="$HARNESS_DIR/runs/$(date +%Y%m%d-%H%M%S)"
mkdir -p "$RUNS_DIR"
echo "runs → $RUNS_DIR"

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
```

- [ ] **Step 2: chmod + commit**

```bash
chmod +x evals/cli-agent/run-suite.sh
git add evals/cli-agent/run-suite.sh
git commit -m "feat(evals): cli-agent suite runner (cases × providers parallel)"
```

---

## PART C — Judge, skill, cleanup

### Task 7: judge template + benchmark skill + README

**Files:**
- Create `evals/cli-agent/judge-template.md`, `evals/cli-agent/SKILL.md`, `evals/cli-agent/README.md`

- [ ] **Step 1: `judge-template.md`**

Create `evals/cli-agent/judge-template.md`:

```markdown
# Verdict — <runs-dir>

For each case × provider, score five dimensions PASS / PARTIAL / FAIL + one-line note.

| case | provider | understood | correct cmd | skill use | nudges | acted | notes |
|------|----------|-----------|-------------|-----------|--------|-------|-------|
| mem.search.basic | claude |  |  |  | 0 |  |  |

## Bugs found
- [ ] <doc gap | card mis-render | CLI bug> — <case> — <fix location> — <fix commit>

## Aggregate
- Per provider: claude X/Y pass, codex …, opencode …
- Per command family: …
```

- [ ] **Step 2: `SKILL.md` — the benchmark skill**

Create `evals/cli-agent/SKILL.md`:

```markdown
---
title: CLI Agent Benchmark
area: testing
tags: [benchmark, cli, agents, integration]
---

# CLI Agent Benchmark

Drive real agents (claude/codex/opencode) in the DEV console (port 7686) through
every CodeTerm CLI command via natural-language asks, then judge their parsed
transcripts. The model is the judge — there is no automated scorer.

## When to use
Verifying that agents understand and correctly execute the CLI, after CLI/doc/
skill changes, or to measure multi-step reliability across providers.

## Run
1. Ensure dev is up: `./build.sh dev --wait`.
2. A batch: `bash evals/cli-agent/run-suite.sh claude,codex,opencode evals/cli-agent/cases/mem.yaml`
3. Transcripts land in `evals/cli-agent/runs/<ts>/<case>-<provider>.md`.
4. Read each transcript and fill `judge-template.md` → `runs/<ts>/verdict.md`.

## Rubric (per case × provider)
1. **Understood** the ask. 2. **Correct command + flags**. 3. **Skill usage**
(used `codeterm` skill / `docs`, didn't hallucinate flags). 4. **Step efficiency**
(nudges to success; 0 = one-shot). 5. **Acted, not described** (real tool_call+result).

## Bug-fix loop
- Unclear docs → fix `src-tauri/prompts/codeterm_cli.md` (shared GA-prompt + skill;
  guarded by `prompt_cli_commands_exist`), rebuild, re-run the case.
- Card mis-render → fix `packages/chat-ui`, rebuild web, re-run.
- CLI bug → fix Rust, rebuild, re-run.

## Safety
Never run `safety: describe-only` cases for real (mesh connect, bridge send,
secrets, system autostart, update install, plugin mutate, config set, theme apply,
destructive mem). `run-case.sh` only sends the prompt; the agent decides — for
describe-only cases the prompt asks it to *explain*, not execute. All bench
workspaces are torn down after each case.
```

- [ ] **Step 3: `README.md`**

Create `evals/cli-agent/README.md`:

```markdown
# evals/cli-agent

Real-daemon LLM benchmark for the CodeTerm CLI. See `SKILL.md` for the protocol
and rubric. Reads transcripts via `codeterm pane chat` / `GET /api/panes/:id/chat`.
Cases live in `cases/*.yaml`; `coverage-check.sh` reports commands without a case.
Unlike the old headless `-p` evals, agents actually execute commands here.
```

- [ ] **Step 4: Register the skill on dev + commit**

```bash
codeterm mem skill add "$(pwd)/evals/cli-agent" || true
git add evals/cli-agent/judge-template.md evals/cli-agent/SKILL.md evals/cli-agent/README.md
git commit -m "docs(evals): cli-agent judge template + benchmark skill + readme"
```

---

### Task 8: evals/ cleanup

**Files:**
- Delete `evals/general-agent/`, `evals/mem-protocol/`
- Modify `evals/README.md`

- [ ] **Step 1: Remove the headless `-p` evals**

```bash
git rm -r evals/general-agent evals/mem-protocol
```

- [ ] **Step 2: Update `evals/README.md`**

Replace the descriptions of the removed sets and add a `cli-agent/` entry. Read `evals/README.md` first, then edit: remove the `general-agent` and `mem-protocol` rows, add:

```markdown
- `evals/cli-agent/` — real-daemon LLM benchmark: drives claude/codex/opencode in
  the dev console through every CLI command; model-judged (see its SKILL.md).
  Replaces the old headless `-p` general-agent / mem-protocol prompt evals.
```

- [ ] **Step 3: Commit**

```bash
git add evals/README.md
git commit -m "chore(evals): drop headless -p general-agent/mem-protocol; document cli-agent"
```

---

## PART D — Run, judge, fix, expand

### Task 9: First real run + judge + bug-fix loop

- [ ] **Step 1: Ensure dev is current**

Run: `./build.sh kill && ./build.sh dev --wait`
Then `curl -s http://127.0.0.1:7686/api/health | jq '.build'` matches `git rev-parse --short HEAD`.

- [ ] **Step 2: Run the first batch across all three providers**

Run: `bash evals/cli-agent/run-suite.sh claude,codex,opencode evals/cli-agent/cases/mem.yaml`
Expected: `runs/<ts>/mem.*-{claude,codex,opencode}.md` transcripts exist and are non-empty.

- [ ] **Step 3: Judge — read each transcript, fill the verdict**

Copy `judge-template.md` → `runs/<ts>/verdict.md`; for every case × provider, read the `.md` transcript and score the 5 dimensions; record bugs found.

- [ ] **Step 4: Fix the known opencode card bug surfaced earlier**

The chat command card renders for claude `Bash` tool calls but not for opencode's
`tool:"bash"` structured path. Investigate why opencode tool calls don't reach
`MessageBubble`'s `tool_call` early-return (check `PaneSessionChat` message
mapping and whether `message.type === "tool_call"` for opencode). Fix in
`packages/chat-ui` (and/or the structured-chat `kind` mapping), rebuild web
(`./build.sh dev`), and confirm via `pane chat` transcript + a Playwright
screenshot of the Boyle-style opencode pane that the CodeTerm card now renders.

- [ ] **Step 5: Apply doc/skill fixes for any command the agents stumbled on**

For each bug attributed to unclear docs, edit `src-tauri/prompts/codeterm_cli.md`,
run `cd cli && cargo test cmd::manifest::tests::prompt_cli_commands_exist`, rebuild
dev, re-run the affected case, update the verdict with the fix commit.

- [ ] **Step 6: Commit fixes + the sample verdict**

```bash
mkdir -p evals/cli-agent/runs/sample
cp evals/cli-agent/runs/<ts>/verdict.md evals/cli-agent/runs/sample/verdict.md
git add -A
git commit -m "test(evals): first cli-agent run + verdict; fix surfaced bugs"
```

---

### Task 10: Expand case coverage to all command families

- [ ] **Step 1: Author remaining case files until coverage is complete**

Create `cases/<family>.yaml` for every family (`pane`, `agent`, `cron`,
`send`, `mesh`, `plugin`, `theme`, `usage`, `sessions`, `config`, `keepawake`,
`bridge`, `paste`, `web`, `service`, `system`, `docs`, plus `mem` graph/skill/worker
subtrees). Each leaf command gets ≥1 case in the Task 5 format. Classify each
`safety: execute | describe-only` per the spec's Safety section (mesh/bridge/
secrets/system/update install/config set/theme apply/plugin mutate/destructive mem
→ describe-only).

- [ ] **Step 2: Coverage gate must report 100% leaf commands covered**

Run: `bash evals/cli-agent/coverage-check.sh`
Expected: `N/N leaf commands covered`, no UNCOVERED list.

- [ ] **Step 3: Full suite run + judge + fix loop (iterate per family)**

Run families in batches: `bash evals/cli-agent/run-suite.sh claude,codex,opencode evals/cli-agent/cases/<family>.yaml`, judge, fix docs/skill/CLI as bugs surface, re-run. Record per-family verdicts.

- [ ] **Step 4: Final multi-step session per provider**

Author `cases/multistep.yaml` with one chained free-form task per provider
(create workspace → spawn sub-agent → send → mem save → mem search →
report), run it, judge multi-step reliability + nudge count.

- [ ] **Step 5: Commit the full catalog + final verdicts**

```bash
git add evals/cli-agent/cases
git commit -m "test(evals): full CLI command coverage + multistep cases"
```

---

## Final verification

- [ ] `cd src-tauri && cargo test service::pane_chat` — green.
- [ ] `cd cli && cargo test cmd::manifest::tests` — green (manifest fresh, doctor passes, `pane chat` present).
- [ ] `bash evals/cli-agent/coverage-check.sh` — 100% leaf commands covered.
- [ ] One end-to-end suite batch produces transcripts + a filled verdict.
- [ ] opencode codeterm-command card renders (Playwright screenshot).
- [ ] `./build.sh check` clean; bump `<!-- revision -->` on any doc touched.
```
