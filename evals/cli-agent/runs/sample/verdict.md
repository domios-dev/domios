# Verdict — sample run 20260608-142753 (claude, opencode)

First real cli-agent benchmark run. Cases: workspace.{create,list}, mem.{search,save,get}.
Read via `codeterm tab chat`; judged by the model on the 5-dimension rubric.

| case | provider | understood | correct cmd | skill use | nudges | acted | notes |
|------|----------|-----------|-------------|-----------|--------|-------|-------|
| workspace.create | claude | PASS | PASS | PASS | 0 | PASS | launched codeterm skill, `workspace create "scratch-bench"` |
| workspace.create | opencode | PASS | PASS | PASS | 0 | PASS | launched skill, created, reported ID |
| workspace.list | claude | PASS | PASS | n/a | 0 | PASS | `workspace list`, counted 25 |
| workspace.list | opencode | PASS | PASS | n/a | 0 | PASS | `workspace list`, counted 25 |
| mem.search | claude | PASS | PASS | PASS | 0 | PASS | 2 searches + summary |
| mem.search | opencode | PASS | PASS | PASS | 0 | PASS | exemplary: 2 searches + 3 `mem get` + `mem graph related` |
| mem.get | claude | PASS | PASS | PASS | 0 | PASS | search → `mem get <real-id>` |
| mem.get | opencode | PARTIAL | PASS | PASS | 0 | PASS | used a real id but a less-relevant item |
| mem.save | claude | PASS | PASS | PASS | 0 | PASS | `mem save --kind fact --title …` in English |
| mem.save | opencode | FAIL | FAIL | — | — | FAIL | said "Noted." — never ran `codeterm mem save` |

## Bugs found
- [x] **read path broken for claude** — `pane_chat_snapshot` called `session_jsonl_path`
  (trait default None; claude overrides `session_file_path`). Fixed.
- [x] **macOS `/var`→`/private/var` cwd symlink** broke claude JSONL path + opencode
  session detection. Fixed: JsProvider canonicalizes cwd before dash-encoding;
  harness uses a non-`/var` sandbox cwd.
- [x] **opencode "remember this" → no `mem save`** (DeepSeek V4 Flash Free): the
  agent treats "Remember this fact…" as conversational memory ("store this in my
  context by acknowledging it") instead of running `codeterm mem save`. Added an
  explicit "remember/save → `codeterm mem save`" cue to the shared
  `codeterm_cli.md` — **re-ran and it did NOT change the behavior** (opencode
  still acknowledged without saving, and skipped loading the skill that turn).
  Conclusion: a **model-capability limitation of DeepSeek V4 Flash Free**, not a
  doc gap. claude executes it correctly with the same skill. The doc cue is kept
  (correct guidance; likely helps stronger models). Action: track as a known
  provider/model weakness, not a codeterm bug.

## Aggregate
- **claude**: 5/5 cases all-PASS, all one-shot. Strong CLI comprehension.
- **opencode** (DeepSeek V4 Flash Free): 4/5 solid (one exemplary with graph
  queries); 1 FAIL (mem.save not executed). Otherwise fetches the skill and uses
  correct commands one-shot.
- Both providers reliably load the `codeterm` skill before acting on novel tasks.
