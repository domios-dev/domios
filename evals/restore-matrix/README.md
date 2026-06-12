# Restore Matrix V2

Focused provider restore eval for dev CodeTerm on port 7686.

This replaces the old broad `evals/session-restore` flow as the acceptance oracle for provider session restore. It creates one workspace named `RESTORE-MATRIX-<runid>`, attempts five panes per system provider (`claude`, `codex`, `opencode`), sends a unique `RESTORE-MARKER-<uuid>` to each active pane, proves the marker materialized in that provider's session store before kill, hard-restarts dev, and requires both API identity checks and a Playwright UI-visible marker assertion.

Run from the repo root with Git Bash:

```bash
bash evals/restore-matrix/run.sh
```

The script targets dev only (`7686`) through `evals/_lib/lib.sh` helpers. It never touches prod `7685`.

Pre-kill gate:

- Every restore-tested pane must prove the marker landed in provider storage before restart: Claude JSONL, Codex rollout JSONL, or OpenCode sqlite/legacy storage.
- If verified send returns ok but the provider store never records the marker after retry, the pane is a send/setup failure, not a restore verdict.
- A provider is skipped only when its probe pane cannot spawn, cannot accept the marker, shows auth/login/account/rate-limit UI, or fails the pre-kill materialization gate.
- If all providers skip or fail setup, the eval exits nonzero before restart.
- Once a provider probe passes the materialization gate, missing markers or identity drift after restart are hard failures.

Acceptance:

- Per active pane: pane id exists after restart, provider is unchanged, session id is preserved when present, no cross-pane sid contamination, and Playwright sees that pane's exact marker in the rendered terminal UI.
- Skipped provider slots are reported explicitly and do not count as pass.
- On success the matrix workspace is closed. On failure it is left open and the manifest path is printed for forensics.
