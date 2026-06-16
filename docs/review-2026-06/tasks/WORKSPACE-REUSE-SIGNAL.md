# WORKSPACE-REUSE-SIGNAL — `workspace create` must say when it reused instead of created

**Executor:** Gpt5.5 docsync (04a7680e) · **Branch:** stabilization · **Effort:** S
**Backlog ref:** arbiter finding 2026-06-12: `workspace create --cwd <path>` silently returns an EXISTING workspace with the same cwd (daemon smart-reuse, src-tauri/src/service/workspace.rs:18-30, JSON already carries `existing: true`) — the requested NAME is ignored and the CLI human output never mentions any of it. A worker broke test isolation on this today.

## Goal
Reuse is loud: human-readable CLI output states it reused an existing workspace (with its real name + id); `--json` consumers keep getting `existing: true` untouched; docs say the behavior exists.

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send "TASK WORKSPACE-REUSE-SIGNAL ACCEPTED, starting" --pane 5c64a651
```

## Evidence (run FIRST; if output differs, report BLOCKED "evidence stale")
```
git log --oneline -1                                   # expected: 2c077721 or a descendant
git grep -n "existing" -- src-tauri/src/service/workspace.rs    # expected: existing: Some(true) around line 30
```

## Steps
1. Find the CLI workspace-create handler (cli/src/main.rs around the workspace dispatch). When the response carries `existing: true`: non-JSON output prints a clear line, e.g. `reused existing workspace <id> '<actual name>' (same cwd); requested name '<requested>' ignored` to STDERR (so scripts parsing stdout are unaffected); `--json` output stays byte-identical to today.
2. Socket path response: verify the socket `workspace.create` reply also carries the existing flag; if it doesn't, surface it the same way the REST path does (small daemon-side addition is in scope ONLY if it's a missing field on the reply struct — no behavior change to the reuse logic itself).
3. docs/CLI.md (regen) + docs/API.md one-line note on the reuse semantics + revision bumps. The codeterm skill folder section on `workspace create` gets the same one-liner (C:/Users/User/.codeterm/skills/codeterm/SKILL.md, bump revision).
4. Tests: cli-level test that a response with existing:true produces the stderr notice and exit 0; existing:false/absent produces no notice.

## Forbidden (beyond the global rules in README.md)
- NO git mutations. Do NOT change the daemon reuse behavior itself (whether create-should-reuse is a separate product decision — this task only makes it visible).

## Report
```
codeterm send "TASK WORKSPACE-REUSE-SIGNAL STATUS ... CHANGED: ... VERIFIED: ... NOTES: ..." --pane 5c64a651
```
