# A2 — Untrack committed binaries, screenshots, and root junk

**Executor:** Haiku or Sonnet pane · **Branch:** stabilization · **Effort:** S
**Backlog ref:** A2 in ../stabilization-backlog.md

## Goal
No binaries or throwaway artifacts tracked in git; `.gitignore` prevents their
return; local junk files deleted.

## Evidence (run FIRST)
```bash
git ls-files | grep -E "codeterm-cli-macos|\.tmp-.*\.png|e2e_proof"
# expected 8 lines: 2 binaries, 5 pngs, e2e_proof.txt
grep -rn "codeterm-cli-macos" scripts/ docs/ .github/ build.sh build.ps1
# expected: ZERO hits. If ANY hit → report BLOCKED, something references the binaries.
```

## Steps
1. `git rm --cached codeterm-cli-macos-aarch64 codeterm-cli-macos-x86_64`
2. `git rm --cached .tmp-burger-a.png .tmp-burger-a-collapsed.png .tmp-burger-a-refined.png .tmp-final-ui.png .tmp-sidebar-tab-refined.png e2e_proof.txt`
3. Append to `.gitignore` (check each pattern isn't already present first):
   `codeterm-cli-macos-*`, `.tmp-*.png`, `e2e_proof.txt`, `chat.json`, `sp.json`, `NUL`
4. Delete local junk (NOT via git — they're untracked):
   `rm -f NUL cli/NUL .qa-read.py`
5. Add one line to `docs/contributing/style.md` (and bump its revision tag):
   "Design artifacts (screenshots, video) go in PR descriptions, never the source
   tree; release binaries come from CI only."
6. Commit: `build: untrack macOS binaries, tmp screenshots, root junk; gitignore them`
   — commit message must note the blobs remain in git history (no history rewrite).

## Verify
```bash
git ls-files | grep -E "codeterm-cli-macos|\.tmp-.*\.png|e2e_proof"   # must: empty
git status --short | grep -E "chat.json|sp.json"                      # must: empty (ignored)
ls NUL cli/NUL .qa-read.py 2>&1                                       # must: all "No such file"
```

## Forbidden
- NO history rewrite (`filter-repo`, `filter-branch`) — explicitly out of scope.
- Do not delete the local binaries from disk if present — only untrack
  (`--cached`); the user may use them.

## Report
```
codeterm send-text "TASK A2 STATUS ... CHANGED: ... VERIFIED: ... NOTES: ..." --pane 5c64a651
```
