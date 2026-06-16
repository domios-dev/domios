# SEND-STDIN — `codeterm send --stdin` (kill the quoting hell)

**Executor:** Gpt5.5 modelvalidate (352d7ecd) · **Branch:** stabilization · **Effort:** S
**Backlog ref:** arbiter pain, 2026-06-12: long dispatch texts with apostrophes/quotes break PowerShell arg quoting (two live incidents this session: clap "unexpected argument" from a mangled message).

## Goal
`codeterm send --stdin --pane ID` (and the `send` alias) reads the message body from stdin instead of argv, so arbitrary text (quotes, apostrophes, newlines, unicode) is sendable without shell-escaping gymnastics.

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send "TASK SEND-STDIN ACCEPTED, starting" --pane 5c64a651
```

## Evidence (run FIRST; if output differs, report BLOCKED "evidence stale")
```
git log --oneline -1                                # expected: 2c077721 or a descendant
codeterm send --help                                # expected: NO --stdin flag yet; TEXT is a positional
```

## Design (decided)
- New flag `--stdin` on the send command (args in cli/src/args/cli.rs, handler in cli/src/main.rs or cmd module — find the existing send path). When set: TEXT positional must be absent (conflict error if both), body = read_to_string(stdin) with trailing-newline preserved as-is.
- Empty stdin → clear error, exit nonzero (no silent empty send).
- Works with the existing `--pane`, `--mesh`, `--raw`/enter semantics unchanged — the body goes through the exact same code path as the positional text afterward.
- Mesh relay: api_map must carry it identically (body already serialized into JSON for the POST — only the SOURCE of the text changes, so api_map likely needs zero change; VERIFY that and say so).
- Regenerate docs/cli-manifest.json + docs/CLI.md per the established flow; one line in docs/SKILLS.md send section.

## Tests
- cli unit (cli/src/...tests): --stdin + positional → conflict error; --stdin with piped text → request body contains the exact text incl. quotes/newlines (follow existing send test patterns if present, else manifest/parse-level tests + one mesh_cli mapping test).
- Manual proof in the report: `echo "it's got 'quotes' and \"doubles\"" | codeterm send --stdin --pane <your own pane id>` delivered intact (paste the received line).

## Forbidden (beyond the global rules in README.md)
- NO git mutations. Only cli/ + the three doc files. No daemon changes.

## Report
```
codeterm send "TASK SEND-STDIN STATUS ... CHANGED: ... VERIFIED: ... NOTES: ..." --pane 5c64a651
```
