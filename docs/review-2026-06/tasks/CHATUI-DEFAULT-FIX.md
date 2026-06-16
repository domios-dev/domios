# CHATUI-DEFAULT-FIX — chat-UI default must apply to daemon/CLI-spawned agents

**Executor:** Gpt5.5 chatui (54499ecb) · **Branch:** stabilization · **Effort:** M
**Backlog ref:** CHATUI-DEFAULT in ../stabilization-backlog.md

## Goal
"Open new agent panes in Chat UI by default" is a server config key honored by EVERY spawn path (UI, CLI, API, orchestrator), not a frontend localStorage flag that only UI spawns read.

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send "TASK CHATUI-DEFAULT-FIX ACCEPTED, starting" --pane 5c64a651
```

## Evidence (run these FIRST; if output differs, report BLOCKED "evidence stale")
```bash
grep -rn "codeterm-chat-ui-default-enabled" src/        # expected: src/lib/chatUiMode.ts (localStorage key)
grep -n "chat_view" src-tauri/src/multiplexer/types.rs  # expected: pane field ~line 137
grep -rn "chat" src-tauri/src/config/schema/mod.rs      # expected: NO existing chat-ui default key
```
Root cause (already established): the setting lives only in frontend localStorage, applied at src/store/multiplexer/index.tsx:308/323 on UI-initiated spawns; daemon spawn path never consults it; dev/prod have separate localStorage so the toggle doesn't even carry over.

## Steps (in order, no skipping, no additions)
1. Config key `ui.default_new_agent_pane_chat: bool` (default false) in src-tauri/src/config/schema/mod.rs, following the existing section/serde patterns.
2. Daemon spawn path (src-tauri/src/service/spawn.rs): at pane creation, set `pane.chat_view = true` when the key is on AND the provider reports chat-UI support via its capability surface. Capability lookup from the provider registry — NO hardcoded provider-name list. Hidden/system panes (dreamer, cron workers) must NOT get chat_view.
3. Frontend: src/lib/chatUiMode.ts reads/writes the config key through the config API instead of localStorage. One-time migration: if the localStorage key exists, push its value to config, then delete the key. UI spawn path consumes the same config value (the daemon now sets chat_view anyway — frontend must not fight it).
4. docs/CONFIG.md: add the key row; bump the revision tag.
5. Tests:
   - Rust unit on the spawn decision: on+capable→true; on+non-capable→false; off→false; on+hidden→false.
   - Non-llm e2e (tests/e2e/): set key via config API → CLI spawn agent → GET pane shows chat_view true; negative: key off → chat_view false. Use the dev-locked conftest.

## Verify (run all; paste real output in the report)
```bash
cargo test --manifest-path src-tauri/Cargo.toml <narrow filter> # must: new unit tests pass (run from PowerShell foreground, 90s cap)
python -m pytest tests/e2e/<new file> -q                        # must: pass against fresh dev
bash build.sh check                                             # must: exit 0
pnpm test:unit                                                  # must: pass (frontend touched)
bash build.sh kill                                              # leave no dev running
```

## Forbidden (beyond the global rules in README.md)
- NO git mutations — report changed paths + proposed commit message; I commit.
- No hardcoded provider-name lists for the capability check.
- No localStorage as a source of truth after this task.
- Don't touch the hidden-pane spawn semantics beyond excluding them from the default.

## Report
```
codeterm send "TASK CHATUI-DEFAULT-FIX STATUS ... CHANGED: ... VERIFIED: ... NOTES: ..." --pane 5c64a651
```
