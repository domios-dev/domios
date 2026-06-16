# E1 — Delete the duplicate TS chat parsers (chat-parsing plan Phase 3)

**Executor:** Sonnet pane · **Branch:** stabilization · **Effort:** S
**Backlog ref:** E1 in ../stabilization-backlog.md · Plan:
docs/superpowers/plans/2026-06-10-chat-parsing-dry-consolidation.md (Phase 3)

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send "TASK E1 ACCEPTED, starting" --pane 5c64a651
```

## Goal
The hand-maintained duplicate JSONL parsers in provider src/ are gone; the host
(plugin.js) remains the single parser. No web/desktop chat regression.

## Evidence (run FIRST — every check must hold or report BLOCKED)
```bash
ls plugins/provider-claude/src/parser.ts plugins/provider-codex/src/parser.ts   # both exist
grep -rn "parseJsonlSession" src/ plugins/ packages/ --include="*.ts" | grep -v "src/parser.ts\|parser.test"
# must: ZERO hits outside the parser files and their tests. ANY other hit = BLOCKED.
grep -rn "from \"./parser\"\|from './parser'" plugins/provider-claude/src/ plugins/provider-codex/src/
# must: list import sites INSIDE each provider's src (likely src/index.ts) — read each to see what it re-exports/uses.
```

## Steps
1. For each provider (claude, codex): if `src/index.ts` imports from `./parser`,
   determine whether that import feeds anything still alive. The plan says the
   chat flow is host-authoritative (`PaneSessionChat.tsx` consumes `res.messages`).
   If the import is part of a still-registered plugin API surface — STOP, report
   BLOCKED with the exact chain.
2. Delete `plugins/provider-claude/src/parser.ts`, `plugins/provider-codex/src/parser.ts`
   and their dedicated test files. KEEP any JSONL fixture files that plugin.js
   tests also load (check tests/ fixtures usage before deleting any fixture).
3. Remove the now-dead imports/exports from each provider's `src/index.ts`.
4. Verify nothing else referenced the deleted modules (re-run the Evidence greps).

## Verify
```bash
pnpm test:plugins                # exit 0
pnpm build                       # tsc green
grep -rn "parseJsonlSession" src/ plugins/ packages/ --include="*.ts"   # zero hits
```
If dev (7686) happens to be up from your previous task, additionally open a chat
pane smoke (cross-pane spec, smoke project only); if dev is down, skip — note it.

## Forbidden
- Do not touch plugin.js files (the surviving parsers).
- Do not delete shared fixtures used by plugin.js tests.
- Git arbiter-only: report paths + proposed msg
  `refactor(providers): delete duplicate TS JSONL parsers (host-authoritative, plan Phase 3)`

## Report
```
codeterm send "TASK E1 STATUS ... CHANGED: ... VERIFIED: ... NOTES: ..." --pane 5c64a651
```
