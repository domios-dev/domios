# CHATUI-DEFAULT-V2 — close the holes the first fix left

**Executor:** Gpt5.5 chatui (54499ecb) · **Branch:** stabilization · **Effort:** M
**Backlog ref:** follows CHATUI-DEFAULT-FIX (c08c2e55). User live-reported on prod: Main pane restores as terminal, the "use chat ui by default?" prompt is a system alert, restore/GA coverage missing. NOTE: prod runs a pre-fix build — that part is release lag, NOT in scope. The three real gaps below are.

## Goal
The chat-UI default applies to the General/Main pane too; the first-run question is an internal dialog, not `window.confirm`; and tests actually cover restore-persistence + GA + the settings toggle.

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send "TASK CHATUI-DEFAULT-V2 ACCEPTED, starting" --pane 5c64a651
```

## Evidence (run these FIRST; if output differs, report BLOCKED "evidence stale")
```
git grep -n "chat_view" -- src-tauri/src/service/general_agent/workspace.rs   # expected: EMPTY (the gap)
git grep -n "window.confirm" -- src/lib/chatUiMode.ts                          # expected: hit in maybeAskDefaultNewAgentPaneToChat
git grep -n "chat_view" -- src-tauri/src/multiplexer/state/snapshot.rs        # expected: lines ~49,132,195 (persistence exists)
```

## Steps (in order, no skipping, no additions)
1. GA/Main pane: in `ensure_general_workspace` (and the existing-workspace repair branch of it), apply the same decision as spawn: config `ui.default_new_agent_pane_chat` on AND the GA provider `supports_chat_ui()` → `pane.chat_view = true`. Careful with the call context (runs BEFORE mux is wrapped — read the fn header comment; config access must work there, pass the value in if needed). Do NOT force-override a pane that already exists with chat_view explicitly set — only set it on first create, and on repair only if the pane has never been toggled (chat_view false + no user intent signal = safe to apply default).
2. Replace `window.confirm` in `maybeAskDefaultNewAgentPaneToChat` with an internal dialog consistent with the app's UI (there are existing modal/confirm patterns in src/components — reuse one; Settings/SchemaRenderer and MemoryPanel use window.confirm too but those are OUT of scope). Two buttons, writes the config key via the existing setDefaultNewAgentPaneToChat, marks prompted either way.
3. Tests:
   - Rust unit: general-workspace creation with default on + capable provider → pane.chat_view true; off → false (mirror the spawn decision tests).
   - e2e (tests/e2e, dev-window deferred run is fine — write now, verify in window): set config on → spawn agent pane → assert chat_view true → hard restart dev (the harness from test_update_resume / session-restore patterns) → assert the SAME pane still has chat_view true after restore. This is the missing restore-persistence proof.
   - Component test for the new dialog (happy-dom harness): renders, yes-path writes config, no-path marks prompted only.
4. docs/CONFIG.md note if behavior wording changes; revision bump.

## Verify (paste real output; dev parts deferred to the next dev window — say so explicitly)
```
cargo test --manifest-path src-tauri/Cargo.toml <narrow filters>   # must: pass
pnpm test:unit                                                      # must: pass
& "C:\Program Files\Git\bin\bash.exe" build.sh check                # must: exit 0
```

## Forbidden (beyond the global rules in README.md)
- NO git mutations. NO touching evals/, restore-matrix files, or dev:7686 until I grant a window.
- Don't convert other window.confirm call sites — only the chat-ui prompt.

## Report
```
codeterm send "TASK CHATUI-DEFAULT-V2 STATUS ... CHANGED: ... VERIFIED: ... NOTES: ..." --pane 5c64a651
```
