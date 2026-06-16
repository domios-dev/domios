# ISSUE-130 — daemon HTTP layer wedges after a panic in the prompt parser

**Executor:** Gpt5.5 modelprobe (77e5f8be) · **Branch:** stabilization · **Effort:** L · **Release-critical**
**Source:** `gh issue view 130` (read it fully first — it has the forensics)

## Goal
A panic in the prompt/hook parser (or any per-request/per-pane worker) can NEVER wedge the daemon's HTTP accept loop. Two parts: (A) make the char-boundary panic unreachable, (B) isolate panics so the listener keeps accepting.

## Step 0 — confirm receipt
```
codeterm send "TASK ISSUE-130 ACCEPTED, starting" --pane 5c64a651
```

## Evidence (run FIRST; report BLOCKED if stale)
```
gh issue view 130
git grep -n "is_char_boundary\|utf8_tail" -- src-tauri/src/agent/prompt.rs   # utf8_tail is already hardened + tested
```

## Scope
- **(A) Char-boundary audit:** `utf8_tail` is already char-boundary-safe (line ~286, with `test_utf8_tail_starts_on_char_boundary`). But #130's panic is in the hook parse path. Audit EVERY byte-index slice (`&s[a..b]`, `s[..n]`, `s[n..]`, `.split_at`, `truncate`) reachable from `hook_detected_parse` / `parse_hook_prompt` and the native+JS `parse_fn` closures in the provider detection path. Any slice on a `&str` at a non-char-boundary panics on multibyte UTF-8 (emoji, box-drawing, CJK in agent TUI output). Replace with char-boundary-safe forms. Add a unit test feeding multibyte text through `hook_detected_parse` that would have panicked.
- **(B) Panic isolation (the real bug):** find WHY a panic there wedges the whole HTTP layer. Likely: the panicking thread holds a `parking_lot` lock (mux/screen_state) that is NOT poisoned-released on unwind, so every subsequent handler blocks forever on that lock → socket stays LISTEN, accepts nothing. Determine the actual mechanism (poisoned/never-released lock vs. a panicking accept task). Fix so a worker panic cannot strand a lock or the accept loop: e.g. wrap the fallible parse in `std::panic::catch_unwind` at the call boundary (it's already a closure boundary), or restructure so the lock guard is dropped before the fallible call. Follow the lock-ordering rules in CLAUDE.md; do not hold a lock across the parse.
- Add a regression test for (B) if feasible (a panicking parse_fn must not leave the lock held / must not propagate to the accept loop).

## Verify (paste real output)
```
cargo test --manifest-path src-tauri/Cargo.toml <narrow filters incl. the new multibyte + panic-isolation tests>   # PowerShell foreground, 90s cap
& "C:\Program Files\Git\bin\bash.exe" build.sh check                  # exit 0
& "C:\Program Files\Git\bin\bash.exe" -c 'bash scripts/check-file-sizes.sh --fail'  # exit 0
```

## Forbidden
- NO git mutations (report paths + proposed commit msg). NO dev:7686 (I own it for release work). No catch_unwind sprinkled everywhere — fix the root lock/boundary, use catch_unwind only at the one natural parse boundary.

## Report
```
codeterm send "TASK ISSUE-130 STATUS ... PART-A: ... PART-B-ROOT-CAUSE: ... CHANGED: ... VERIFIED: ..." --pane 5c64a651
```
