# Render codex structured apply_patch edits as diffs — plan + task

<!-- revision: 1 -->

## Problem
Newer Codex (cli 0.141, gpt-5.5) records file edits as **structured** events in its session JSONL, but CodeTerm's chat view does not always render them as a diff — an edit can show up as a plain command / not parse as a code change, even though the JSONL has the full patch. Observed live during the mem-append-fix worker (real session below).

## What the JSONL actually contains (verified)
Session: `~/.codex/sessions/2026/06/23/rollout-2026-06-23T21-02-21-019ef5a5-c1b3-7941-a356-4f61db8e5970.jsonl`
- Edits are a `payload.type == "custom_tool_call"`, `name == "apply_patch"`, whose `input` is the patch text:
  ```
  *** Begin Patch
  *** Update File: cli/src/cmd/mem/mod.rs
  @@
       let text = resp.text().unwrap_or_default();
  -    <old line>
  +    <new line>
  *** End Patch
  ```
  (also `*** Add File:` / `*** Delete File:` forms exist).
- Followed by `payload.type == "patch_apply_end"` with `stdout: "Success. Updated the following files:\nM <file>\n..."`, plus `success`, `changes`, `call_id`, `turn_id`.
- Search-only `rg -n ...` calls are `exec_command` (NOT edits) — must NOT be rendered as edits.

## Why it doesn't render
- The renderer EXISTS and works: `packages/chat-ui/src/MarkdownBody.tsx::buildDiffLines(edits: EditPair[])` draws the green/added / red/removed lines (the diff the user already sees for SOME edits).
- The gap is in the PARSE step that turns the codex transcript into chat items / `EditPair[]`. The codex provider plugin `plugins/provider-codex/src/plugin.ts` only detects the LEGACY text prompt ("Would you like to make the following edits"); the structured `apply_patch` custom_tool_call + `patch_apply_end` path is not converted into an edit/diff item in all cases. `src-tauri/src/service/tab_chat/` likewise has no `apply_patch`/`patch_apply_end` handling.

## Plan
1. **Trace the pipeline** end to end: codex JSONL → (Rust `tab_chat` parse OR the frontend `src/components/ChatView/ChatView.tsx` / `packages/chat-ui` transcript parser) → chat item → `MessageBubble` → `buildDiffLines`. Find exactly where an `apply_patch` custom_tool_call should become an edit item and where it currently falls through.
2. **Parse `*** Begin Patch` → `EditPair[]`**: write/extend a parser that turns the apply_patch `input` into `{ old, new }` edit pairs per hunk per file (handle Update/Add/Delete File). Reconcile with `patch_apply_end.changes`/`stdout` for the file list + success state.
3. **Render**: feed those edits into the existing `buildDiffLines`/diff component so every codex structured edit shows as a labeled `Edited <file> (+N -M)` diff bubble — same as the cases that already work. Search-only shell (`rg`, `cat`, `ls`) stays a normal command, never an edit.
4. **Fixtures + tests**: add a fixture in `packages/chat-ui/src/providerTranscriptFixtures.testdata.ts` built from the REAL session above (the `apply_patch` on `cli/src/cmd/mem/mod.rs`) and assert it renders as a diff (extend `MarkdownBody.diff.test.tsx` / `MessageBubble.providers.test.tsx`). Add a negative test: an `rg`/`exec_command` does NOT render as an edit.

## Gates
- `npm run typecheck` clean; the new + existing chat-ui transcript tests green.
- Playwright web-mode (if dev up) showing a codex structured edit rendering as a diff; else leave the live check to the Orchestrator.
- Files < 1000 lines.
