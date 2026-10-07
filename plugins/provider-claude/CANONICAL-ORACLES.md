# Canonical DEV verification

These are requested observations, not executed results. Run after a real merge
in the canonical checkout and the owner DEV build. Keep Codex on 0.160.0.
No lane build, extra DEV instance or PROD 7685 operation belongs to this leg.

## Canonical gates

Run serially through the authorized gateway runner in a Team utility shell:

```powershell
.\build.ps1 check-tests
.\build.ps1 test-one app claude_permission_hook_ --wait-build
.\build.ps1 test-one app claude_context_feed_ --wait-build
.\build.ps1 test-one plugin claude_context_usage_ --wait-build
.\build.ps1 test-one plugin claude_statusline_ --wait-build
.\build.ps1 test-one plugin parity_ --wait-build
.\build.ps1 test-one plugin test_claude_does_not_inject_session_id --wait-build
```

All commands must exit zero with nonzero matching counts. The permission filter
contains twelve tests, context app filter two, native usage filter two and native
statusline filter one. The parity filter includes three new context cases plus
the permission-capability case. Record raw exits/counts; compilation is not a
runtime pass. On Windows the desktop fixture cases must execute, not be ignored.

```bash
bash scripts/plugin-tests.sh provider-claude
```

This runs eleven files, including the new context/chaining file. No provider
test invokes Claude or modifies the user's global settings.

## DEV preflight and owned-hook refresh

In canonical Git Bash:

```bash
source evals/_lib/lib.sh
dev_get /api/health
git rev-parse HEAD
claude --version
claude --help
dc --as-tab "$DEV_OWNER_TAB" team show "$DEV_TEAM"
dc --as-tab "$DEV_OWNER_TAB" config get agent.full_access
dc --as-tab "$DEV_OWNER_TAB" config get agents.claude
```

Set DEV_OWNER_TAB and DEV_TEAM to the running DEV Orchestrator's explicit ids.
Health must identify the merged SHA and DEV port 7686. Save the two config
responses for exact restoration. Record Claude version and supported permission
modes; installed 2.1.292 uses `manual`, not the older `default` spelling.

Save a byte copy/hash of `~/.claude/settings.json` and its parsed statusLine.
This refresh is the owner-approved, one-time trusted-content change described in
[SIDE-CHANNELS.md](SIDE-CHANNELS.md): the installed owned PermissionRequest
command discarded stdout. Attach that note before merge. Run once:

```bash
dc --as-tab "$DEV_OWNER_TAB" integration install --provider claude
dc --as-tab "$DEV_OWNER_TAB" integration status --provider claude
dc --as-tab "$DEV_OWNER_TAB" config set agents.claude.permission_hook_decisions true
dc --as-tab "$DEV_OWNER_TAB" config set agent.full_access false
```

Read the installed PermissionRequest command: the marker remains inside it,
URL/token/tab come only from environment, curl stdout is preserved, and no
foreign hook or statusLine changed. If Claude asks for trust, the owner grants
it. Do not call a suppressed/untrusted hook a decision-path pass.

## B1 actual allow, deny and unanswered fallback

Create a throwaway directory ORACLE_DIR and three task files in the DEV owner's
AgentPad: `allow.md`, `deny.md`, `timeout.md`. Each asks for exactly one tool call
that writes a different marker file in ORACLE_DIR, then reports its tool result.
Use a tool/command not covered by the user's existing allow rules; no bypass
flag, standing grant or automatic permission mode is permitted. Give each task
a distinct random marker. Save paths ALLOW_TASK, DENY_TASK and TIMEOUT_TASK.

For example, generate the disposable inputs without sending inline prompt text:

```bash
RUN_ID="$(python3 -c 'import uuid; print(uuid.uuid4())')"
PAD="$(dc --as-tab "$DEV_OWNER_TAB" pad path)"
ORACLE_DIR="$(pwd -W 2>/dev/null || pwd)/.codeterm/tmp/claude-oracle-$RUN_ID"
mkdir -p "$ORACLE_DIR"
for CASE in allow deny timeout; do
  cat > "$PAD/$CASE.md" <<EOF
Use Bash exactly once to execute: printf '%s' '$RUN_ID-$CASE' > '$ORACLE_DIR/$CASE.marker'
Do not use another tool or retry a refused command. Report the tool's actual result.
EOF
done
ALLOW_TASK="$PAD/allow.md"
DENY_TASK="$PAD/deny.md"
TIMEOUT_TASK="$PAD/timeout.md"
```

For each task, create a roleless interactive agent, preserving Team membership:

```bash
dc --as-tab "$DEV_OWNER_TAB" agent spawn claude --team "$DEV_TEAM" --cwd "$ORACLE_DIR" --task-file "$ALLOW_TASK" --args --permission-mode manual
dc tab status --tab "$ALLOW_TAB" --json
dc tab inspect --tab "$ALLOW_TAB" --lifecycle --json
dc notify list --json
dc tab respond 1 --tab "$ALLOW_TAB"
```

Read ALLOW_TAB from that spawn response. Await the actual structured prompt
event before responding; verify option 1 is the declared Allow. Before the
answer, require an action_needed bell record with the same request identity and
two choices, plus delivery on configured Telegram/push surfaces. After it, the
marker file must contain exactly its assigned token and Claude must show a
successful tool result. The notification's buttons must retire everywhere.

Repeat the spawn with DENY_TASK and save DENY_TAB. Answer Deny from the existing
notification (Telegram, ring or bell) rather than sending text. Require a
provider tool-denied result, no marker file and notification retirement. A
second choice on the same record must not change the result.

Repeat with TIMEOUT_TASK and save TIMEOUT_TAB. Leave the external request
unanswered. Await the bounded request deadline and the provider's own permission
prompt event/screen, without a polling loop or readiness sleep. Require no
marker file, no decision, and retired external notification. Inspect:

```bash
dc tab get-text --tab "$TIMEOUT_TAB"
dc tab status --tab "$TIMEOUT_TAB" --json
dc notify list --json
```

The tool must remain unexecuted until the owner answers Claude's own prompt.
For concurrency, ask a separate agent for two parallel, independently gated
tool calls. Require two live notification ids; answer the second Deny and leave
the first live, then Allow the first. Only its marker may exist. Cancel a
separate request/tab and require every cancelled request's notice to retire.
Serial tool calls do not prove concurrency; the focused broker/fixture gate is
the deterministic overlap oracle.

## B3 per-launch settings and JSONL meter

```bash
dc --as-tab "$DEV_OWNER_TAB" config set agents.claude.context_feed true
dc --as-tab "$DEV_OWNER_TAB" agent spawn claude --team "$DEV_TEAM" --cwd "$ORACLE_DIR" --args --codeterm-statusline
dc tab inspect --tab "$CONTEXT_TAB" --json
```

Record CONTEXT_TAB from the spawn result. Complete a normal turn, enter chat
mode, and observe the existing mux snapshot. Require tab.context's used_tokens
to equal input + cache-read + cache-creation for the current observation, with
the provider's context_window_size and USD cost/null. Only the four contract
fields may be present. The original statusline output must still render.
Compare the saved global statusLine and settings bytes after launch: launching
must not write the file (the separately approved hook refresh is already done).

Launch a second tab without `--codeterm-statusline`, with chat mode active.
After a turn, require source claude-jsonl and current message.usage input counts,
not a cumulative sum across prior messages. Supply context_window_tokens only
if the owner has authoritative evidence of that session's denominator; without
it or a reported window, window_tokens is zero and Helm hides the meter. Do not
substitute a model-name guess. A wrong session/nonce observation must not change
the snapshot. A current_usage:null statusline report must clear context.

## B2 isolated fresh interactive session-id comparison

Keep STRIPPED_PTY_ENV unchanged: CLAUDE_CODE_SESSION_ID and
CLAUDE_CODE_CHILD_SESSION remain removed in both cases. Use the same directory,
model, manual permission mode and a task file that asks only to echo a unique
marker, with no tools. First run the baseline:

```bash
cat > "$PAD/session-id.md" <<EOF
Reply with exactly this marker: $RUN_ID-session-id. Do not call tools.
EOF
BASELINE_TASK="$PAD/session-id.md"
SESSION_ID_TASK="$BASELINE_TASK"
SID="$(python3 -c 'import uuid; print(uuid.uuid4())')"
```

```bash
dc --as-tab "$DEV_OWNER_TAB" agent spawn claude --team "$DEV_TEAM" --cwd "$ORACLE_DIR" --task-file "$BASELINE_TASK" --args --permission-mode manual
dc tab inspect --tab "$BASELINE_TAB" --lifecycle --json
dc sessions get "$BASELINE_SID" --json
```

Read BASELINE_TAB and its claimed BASELINE_SID from the actual responses.
Require a real user marker and assistant response in that exact metadata
file_path. Exit Claude normally with its own `/exit`, verify shell return and
the persisted transcript still exists and contains both records.

Mint a new UUID SID and change only the explicit fresh-session argument:

```bash
dc --as-tab "$DEV_OWNER_TAB" agent spawn claude --team "$DEV_TEAM" --cwd "$ORACLE_DIR" --task-file "$SESSION_ID_TASK" --args --permission-mode manual --session-id "$SID"
dc tab inspect --tab "$SESSION_ID_TAB" --lifecycle --json
dc sessions get "$SID" --json
```

Require the claimed session to equal SID, the exact user and assistant markers
in its own transcript while live, normal interactive `/exit`, and that same
transcript surviving afterwards. Record both metadata paths, record ids and
file sizes before/after exit. A bound tab alone, a print-mode run or a file for
another session is not a pass. Injection remains off in this lane; only a
successful live comparison justifies a later native/JS capability change.

Close only the throwaway DEV tabs via `dc tab close --tab ID` and restore the
saved DEV config values. Keep the owner-approved hook refresh; do not overwrite
the user's settings with the pre-refresh copy or undo foreign concurrent edits.
