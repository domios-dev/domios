# PR #54 — macOS verification (handoff from BLACKSTAR Worker)

**Branch:** `chore/cli-dev-bin-and-mem-slim` (already pulled if you're reading this)
**Issues bundled:** #47, #50, #51, #55
**What BLACKSTAR did:** all code + 910 Rust tests + 49 frontend tests, pushed.
**What you do here:** verify the macOS-only paths nobody could prove on Windows.

If you're a Claude pane spawned for this task: read this file end-to-end, then execute the steps **strictly in order**, reporting after each step. End with `codeterm agent report`.

---

## Step 0 — Confirm you're on the branch and built

```bash
cd /Users/rollacode/Developer/codeterm
git fetch origin
git checkout chore/cli-dev-bin-and-mem-slim
git pull --ff-only
git log --oneline -5
```

Expected top commit: `test+fix(train): cover paste helpers, codex shapes, cli_shim global flags`.

```bash
./build.sh dev
# wait ~10s, then:
curl -s http://127.0.0.1:7686/api/health | jq '{build,build_number,version}'
```

Build hash must match `git rev-parse HEAD`. If not, dev didn't pick up — kill and rerun.

```bash
./build.sh test 2>&1 | tail -3
```

Expected: `910 passed; 0 failed; 1 ignored`.

---

## Step 1 — Verify #51 (CLI argv silent drop on macOS)

The dev GUI binary now has `forward_to_cli_if_invoked()` running before Tauri init.

Find the dev GUI binary path:

```bash
DEV_GUI=$(find ./src-tauri/target/debug -maxdepth 2 -name codeterm -type f 2>/dev/null | head -1)
echo "Dev GUI binary: $DEV_GUI"
file "$DEV_GUI"
```

Should be a Mach-O executable. Then probe:

```bash
"$DEV_GUI" identify
echo "Exit: $?"
```

**Expected:** prints JSON identify (proves the shim execs into `~/.codeterm-dev/bin/codeterm`).
**Failure mode (regression):** prints daemon INFO logs + nothing useful, exits 0 silently.

The TDD-found bug — global flags before subcommand:

```bash
"$DEV_GUI" --json mem search test 2>&1 | head -3
"$DEV_GUI" --mesh BLACKSTAR identify 2>&1 | head -3
```

**Expected:** both produce CLI output (JSON / object). Not silent.

Now reproduce the original-issue scenario — symlinking the GUI binary as the CLI shim:

```bash
mkdir -p ~/.local/bin
ln -sf "$DEV_GUI" ~/.local/bin/codeterm-shim-test
~/.local/bin/codeterm-shim-test identify
```

**Expected:** identify JSON, NOT GUI launch.

Cleanup:

```bash
rm -f ~/.local/bin/codeterm-shim-test
```

---

## Step 2 — Verify #55 (image paste / drop placeholder)

Dev codeterm should already be running (port 7686).

**A. Screenshot in clipboard (in-memory bytes path) — should still work as before.**

1. `Shift+Cmd+Ctrl+4` → select a region. Bytes go to clipboard.
2. In dev codeterm UI, open a Claude pane (or any agent), focus chat input.
3. `Cmd+V`. Attachment chip appears.
4. Send the message with text "what do you see in the image".
5. After Claude responds, check `~/.claude/image-cache/<session-id>/` — newest file should be the actual screenshot, not a 1024×1024 generic icon.

**B. Cmd+C of a Finder file (the broken case before this PR).**

1. In Finder, pick any real PNG file on Desktop (or take one with `Shift+Cmd+4`, saves to Desktop).
2. `Cmd+C` on that file in Finder.
3. In dev codeterm chat input, `Cmd+V`.
4. Attachment chip should show the file. Send with prompt "what's in this image".
5. Check `~/.claude/image-cache/<session-id>/` — newest file should match the source file's real bytes (size, dimensions, content). **Regression mode:** 1024×1024 RGBA placeholder.

**C. Drag-drop from Finder.**

1. Drag a PNG from Finder onto the chat input area.
2. Should attach via Tauri native drop (real path, real bytes).

Report: A/B/C each PASS or FAIL with file size + dimensions of the resulting cached image.

---

## Step 3 — Mesh round-trip (BLACKSTAR ↔ Mac)

Just confirm mesh works both ways from the new dev daemon:

```bash
codeterm --mesh BLACKSTAR identify
```

**Expected:** BLACKSTAR identify JSON. Confirms two-way mesh works on the new build.

If you can — try a write op (this is what BLACKSTAR couldn't do because Mac was on old 1.1.16):

```bash
codeterm --mesh BLACKSTAR pane new --workspace WS_ID --title "from-mac" --json
```

(Pick any non-protected workspace ID from `codeterm --mesh BLACKSTAR workspace list`.)
**Expected:** pane created remotely, returns pane_id.

---

## Step 4 — Telegram bridge ping (signal-passing test, no real data)

```bash
codeterm bridge send "Mac worker reporting in for PR #54" --to 186809246 --service telegram
```

**Expected:** `ok true`. Andrey gets the ping. Done.

If your Mac doesn't have telegram bridge configured yet, just print the config status and skip:

```bash
codeterm config get integrations.telegram_enabled || echo "telegram not wired on this host — skip"
```

---

## Step 5 — Report back

Use `codeterm agent report` with a structured summary:

- Step 0: build OK / FAIL
- Step 1 (#51): identify OK / regress, --json forward OK / regress, --mesh forward OK / regress
- Step 2 (#55): A OK / FAIL, B OK / FAIL, C OK / FAIL — with file sizes for each
- Step 3 (mesh): outbound identify OK / FAIL, write op OK / FAIL
- Step 4 (telegram): OK / SKIPPED
- Any unexpected: short note

If anything regresses, do NOT auto-commit a fix. Report findings; BLACKSTAR Worker (Andrey's main session) decides next.
