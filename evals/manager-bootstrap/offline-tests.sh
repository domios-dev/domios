#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FIXTURES="$ROOT/evals/manager-bootstrap/fixtures"
RUN="$ROOT/evals/manager-bootstrap/run.sh"
source "$ROOT/evals/manager-bootstrap/helpers.sh"

expect_failure() { "$@" >/dev/null 2>&1 && return 1 || return 0; }

[ "$(first_assistant < "$FIXTURES/chat.json")" = 'Planner manager-bootstrap-eval not yet established' ]
[ "$(first_assistant <<< '[{"role":"assistant","content":"array envelope"}]')" = 'array envelope' ]
expect_failure first_assistant <<< '{"messages":"not-a-list"}'
[ "$(provider_id system < "$FIXTURES/providers.json")" = native-agent ]
[ "$(provider_id external < "$FIXTURES/providers.json")" = external-agent ]
expect_failure provider_id system <<< '{"providers":[]}'
[ "$(new_team_id "$(<"$FIXTURES/teams-before.json")" < "$FIXTURES/teams-after.json")" = team_new ]
expect_failure new_team_id '[]' < "$FIXTURES/teams-after.json"
[ "$(pair_pane_id orchestrator < "$FIXTURES/pair.json")" = pane_orchestrator ]
[ "$(pair_pane_id planner < "$FIXTURES/pair.json")" = pane_planner ]
[ "$(spawned_pane_id < "$FIXTURES/spawn.json")" = pane_worker ]
[ "$(staffed_pane_id < "$FIXTURES/member.json")" = pane_staffed ]

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
grep -q 'dev_post_as_pane' "$RUN"
echo "manager-bootstrap offline parser harness: PASS"
