#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FIXTURES="$ROOT/evals/manager-bootstrap/fixtures"
RUN="$ROOT/evals/manager-bootstrap/run.sh"
source "$ROOT/evals/manager-bootstrap/helpers.sh"
expect_failure() { "$@" >/dev/null 2>&1 && return 1 || return 0; }
assert_tracked_fixtures() {
  local fixture
  [ "${MANAGER_BOOTSTRAP_EXPORTED:-}" = 1 ] && return 0
  while read -r fixture; do
    git -C "$ROOT" ls-files --error-unmatch "evals/manager-bootstrap/fixtures/${fixture#\$FIXTURES/}" >/dev/null
  done < <(rg -o '\$FIXTURES/[[:alnum:]_.-]+\.json' "$BASH_SOURCE" | sort -u)
}
assert_exported_tree() {
  [ "${MANAGER_BOOTSTRAP_EXPORTED:-}" = 1 ] && return 0
  local exported
  exported="$(mktemp -d)"
  git -C "$ROOT" archive HEAD | tar -x -C "$exported"
  MANAGER_BOOTSTRAP_EXPORTED=1 bash "$exported/evals/manager-bootstrap/offline-tests.sh"
  rm -rf "$exported"
}

assert_tracked_fixtures

[ "$(first_assistant < "$FIXTURES/chat.json")" = 'Planner manager-bootstrap-eval not yet established' ]
[ "$(first_assistant <<< '[{"role":"assistant","content":"array envelope"}]')" = 'array envelope' ]
expect_failure first_assistant <<< '{"messages":"not-a-list"}'
[ "$(provider_id system < "$FIXTURES/providers.json")" = native-agent ]
[ "$(provider_id external < "$FIXTURES/providers.json")" = external-agent ]
expect_failure provider_id system <<< '{"providers":[]}'
[ "$(new_team_id "$(<"$FIXTURES/teams-before.json")" < "$FIXTURES/teams-after.json")" = team_new ]
expect_failure new_team_id '[]' < "$FIXTURES/teams-after.json"
[ "$(pair_tab_id orchestrator < "$FIXTURES/pair.json")" = tab_orchestrator ]
[ "$(pair_tab_id planner < "$FIXTURES/pair.json")" = tab_planner ]
[ "$(spawned_tab_id < "$FIXTURES/spawn.json")" = tab_worker ]
[ "$(staffed_tab_id < "$FIXTURES/member.json")" = tab_staffed ]
mux_absent tab_missing < "$FIXTURES/mux.json"
expect_failure mux_absent tab_existing < "$FIXTURES/mux.json"

IDLE_STARTER='You are a newly started agent. Wait for instructions from the user or your orchestration parent.'
rg -Fq 'IdleWakeup' "$ROOT/src-tauri/crates/codeterm-plugin/src/provider.rs"
rg -Fq "$IDLE_STARTER" "$ROOT"/plugins/provider-*/src/plugin.ts
rg -Fq 'starter_prompt_for_assignment' "$ROOT/src-tauri/src/service/orchestrator/spawn.rs"
rg -Fq 'starterPrompt === "idle_wakeup"' "$ROOT/plugins/provider-codex/src/plugin.ts"
rg -Fq 'starterPrompt === "idle_wakeup"' "$ROOT/plugins/provider-opencode/src/plugin.ts"
python - <<'PY'
messages = [
    {"id": "repeat-a", "content": "same inbound instruction"},
    {"id": "repeat-b", "content": "same inbound instruction"},
]
assert len({message["id"] for message in messages}) == 2
assert len(messages) == 2
PY
FAKE="$(mktemp -d)"; trap 'rm -rf "$FAKE"' EXIT
printf '%s\n' '#!/usr/bin/env bash' "python -c 'print(\"[\" + \",\".join(\"0\" for _ in range(200000)) + \"]\")'" 'printf complete > "$1"' > "$FAKE/producer"
chmod +x "$FAKE/producer"
PAYLOAD="$(capture_json "$FAKE/producer" "$FAKE/complete")"
expect_failure provider_id system <<< "$PAYLOAD"
[ "$(<"$FAKE/complete")" = complete ]

printf '%s\n' '#!/usr/bin/env bash' 'exit 1' > "$FAKE/curl"; chmod +x "$FAKE/curl"
set +e; out="$(PATH="$FAKE:$PATH" bash "$RUN" 2>&1)"; rc=$?; set -e
[ "$rc" -eq 7 ] && grep -q 'dev 7686 unavailable' <<<"$out"
! rg -q '[s]leep' "$ROOT/evals/manager-bootstrap"
! rg -q 'claude|opus|fable|runner\\.py' "$ROOT/evals/manager-bootstrap"
grep -q 'dev_post_as_tab' "$RUN"
assert_exported_tree
echo "manager-bootstrap offline parser harness: PASS"
