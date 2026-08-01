# Restore Matrix V2

Focused provider restore eval for dev CodeTerm on port 7686.

This replaces the old broad `evals/session-restore` flow as the acceptance oracle for provider session restore. It creates one workspace named `RESTORE-MATRIX-<runid>`, attempts N tabs per system provider (`claude`, `codex`, `opencode`, `cursor`), sends a unique `RESTORE-MARKER-<uuid>` to each active tab, proves the marker materialized in that provider's session store before kill, hard-restarts dev, and requires API identity, `/chat` isolation, and a scoped ChatView UI-visible marker assertion.

Run from the repo root with Git Bash:

```bash
bash evals/restore-matrix/run.sh                                  # defaults below
RM_PROVIDERS=cursor RM_PER_PROVIDER=1 bash evals/restore-matrix/run.sh  # debug one provider
```

Tunables (env, with defaults):

- `RM_PROVIDERS` (`claude codex opencode cursor`) — which providers to test; narrow it to debug one at a time.
- `RM_PER_PROVIDER` (`3`) — tabs per provider. Three proves same-provider multi-tab isolation without making the default matrix unbounded.
- `RM_MATERIALIZE_TIMEOUT` (`60`) — seconds to wait for the marker to land in the provider store before a tab is a fast send/setup failure. Was a 240s×2 hang per tab, which made a full claude run take 30–40 min.

The script targets dev only (`7686`) through `evals/_lib/lib.sh` helpers. It never touches prod `7685`.

## Known flake: Claude first-turn JSONL persistence is not reliably prompt

The pre-kill gate requires the marker in the provider store because that is what `--resume` reads after restart — a marker only on screen would not actually survive a restart. Codex (rollout JSONL) and OpenCode (sqlite) persist promptly. **Claude does not reliably flush the first conversation turn to its `~/.claude/projects/<enc>/<sid>.jsonl` within the window**: observed both a 111-byte sidecar containing only the `ai-title` line (reply rendered on screen, turn never persisted) and a project dir that never appeared at all, while other runs persisted a full ~33 KB JSONL within seconds. This is why claude tabs are the ones that historically hung the gate. Until the persistence behavior is understood (CodeTerm session handling vs. Claude Code flush cadence — tracked as a finding), prefer `RM_PROVIDERS="codex opencode"` for a fast deterministic run, and treat the claude lane as best-effort. `evals/restore-matrix/debug-claude-persist.sh` is a throwaway single-tab latency probe used to characterize this.

Pre-kill gate:

- Every restore-tested tab must prove the marker landed in provider storage before restart: Claude JSONL, Codex rollout JSONL, OpenCode sqlite/legacy storage, or Cursor's `.cursor/projects/<slug>/agent-transcripts/<sid>/<sid>.jsonl` after the workspace-trust marker exists.
- If verified send returns ok but the provider store never records the marker after retry, the tab is a send/setup failure, not a restore verdict.
- A provider slot is skipped only when it cannot spawn, cannot accept the marker, shows auth/login/account/rate-limit UI, or fails the pre-kill materialization gate.
- If all providers skip or fail setup, the eval exits nonzero before restart.
- Once a slot passes the materialization gate, missing markers or identity drift after restart are hard failures.

Acceptance:

- Per active tab: tab id exists after restart, provider is unchanged, session id is preserved when present, no cross-tab sid contamination, `/chat` contains only that tab's marker, and Playwright sees that tab's exact marker in the rendered ChatView UI.
- Skipped provider slots are reported explicitly and do not count as pass.
- On success the matrix workspace is closed. On failure it is left open and the manifest path is printed for forensics.
