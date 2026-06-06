# Handoff: web mirror — continue on Mac-Air (dijkstra)

**From:** opus pane on BLACKSTAR (Windows, dev console 7686)
**To:** dijkstra tab on MacBook Air
**Branch:** `feat/web-mirror-fixes` (off `develop`)
**Date context:** all "today" = 2026-06-06.

## FIRST THING: merge our work into ONE branch

Andrey's explicit instruction: you did the pluginization work (provider
parsers → TS packages `@codeterm/provider-claude`, `@codeterm/provider-codex`,
`@codeterm/chat-ui`, `@codeterm/chat-core`); I did the web mirror + composer
work on `feat/web-mirror-fixes`. **Merge both efforts into a single shared
branch and continue there.** Pick whichever direction is cheaper
(`feat/web-mirror-fixes` ← your plugin branch, or merge both into a fresh
`feat/web-mirror-plugins`), resolve, run the gates below, and tell Andrey
which branch is canonical. My changes touch `PaneSessionChat.tsx` (which
imports your parser packages) — review those import sites when merging;
I did NOT modify anything inside the provider packages themselves.

Then: **run your dev console with the merged build** so Andrey can hit it
from his phone, and we live-test cross-machine peer switching together
(BLACKSTAR dev ↔ Air dev over the mesh, see "what remains" #2).

## What this branch contains (all verified on Windows)

### Web mirror core (done, tested)
- `src/lib/bridge.ts` — invoke()→REST + listen()→SSE mapping (web mode).
  Honest loud rejects for browser-meaningless commands.
- `src/lib/webApi.ts` — IS_WEB, per-host token storage, webFetch/webJson,
  **webUpload()** (multipart → `POST /api/upload`).
- `src/lib/webEvents.ts` — SSE consumer of `/api/events/stream` + poll tick.
- WebGate token gate: `#token=` / `?token=` share links auto-consume +
  URL redaction; `codeterm web link [--host|--port|--query|--json]` CLI
  (emits tailscale-IP fragment links; uses config api.token, NEVER the
  short-lived signed mesh token — that was a real bug, don't regress it).
- Server-side spawn (`POST /api/agents/spawn`) used by web spawnAgentTab;
  chat-raw routes (`/api/panes/{id}/chat-raw/{start,stop,backfill}`)
  return the exact Tauri command shapes; both chat sinks now
  `emit_everywhere` so deltas reach SSE.
- PTY-over-HTTP long-poll (`/api/pty/{id}/{read,write,resize}`).
- Web chrome strip: no Titlebar in web, floating ☰, WebPeerPicker +
  KeepAwakeTray live in a sidebar top block; `100dvh` + viewport meta
  (`user-scalable=no, viewport-fit=cover, interactive-widget=resizes-content`);
  `useKeyboardInset` hook (iOS visual-viewport fallback) — **known
  remaining bug**: on iOS Safari the composer still doesn't sit flush
  above the keyboard (huge gap, Andrey's screenshot) — see remains #5.

### Composer (today's work, done)
- `src/components/Terminal/ComposerActions.tsx` — attach-file button
  (hidden input), voice-note recording (MediaRecorder; webm/opus or mp4
  on iOS), explicit send button. Rendered on BOTH web and desktop
  (Andrey: «не дели») in `PaneSessionChat`.
- `storeBlob()` branches: web → `webUpload` (daemon `uploads/` dir),
  desktop → existing `save_temp_file` Tauri command.
- Web drag-n-drop upload (`useWebFileDrop`); Tauri keeps native
  `useTauriFileDrop`.
- Backend `POST /api/upload` (axum multipart feature enabled in
  Cargo.toml, 200MB body cap, filename sanitizer, uuid prefix) →
  `{path, size}`. Files land in `<data_dir>/uploads/`.
- Fixed pre-existing Windows bug in `/api/serve-file`: canonicalize()
  yields `\\?\` verbatim paths, the prefix guard compared against plain
  data_dir() → every request 403'd. Both sides canonicalized now.

### Agent-boot send-loss fix (today, important)
Symptom: `POST /panes/{id}/send` returned 200 but text NEVER appeared in
a freshly spawned agent pane — a booting claude TUI silently swallows
bracket-paste. Deterministic repro existed (`probe` script, now deleted).
Fix: `Pane.agent_booting` runtime flag (serde skip), set synchronously in
`launch_agent_in_background` before the thread spawns, cleared on every
exit path; `send_text_when_ready()` in `service/pty_io.rs` waits it out
(90s cap, then honest error). Wired into the API `/send` route AND the
Tauri `chat_send_text` command. The desktop chat had the same bug.

### Tests (Playwright, all green on Windows at handoff)
- 12-web: 12 tests — token gate, share links, peer switch via MeshStub,
  mobile viewport, chrome strip, hamburger flow, hermetic chat test
  (spawns own sonnet pane: live SSE reply + reload-history + attach chip;
  scoped by `[data-pane-id]`), upload round-trips in 11-api.
- 01-smoke asserts Titlebar present in Tauri (12-web asserts absent in web).
- Harness fixes you'll care about on macOS: `BASH_EXE` pinning in
  `helpers/dev-console.ts` is Windows-only (plain `bash` elsewhere);
  `connectUi` accepts xterm OR chat-composer (a pane left in chat-view
  hides its xterm — bit us hard).
- Contract: `pnpm test:pw` locally before PR; no CI by decision.
- Run order gotcha: `pnpm build` after frontend changes — the daemon
  serves `/web` from `dist/`.

### Known flake (pre-existing, NOT mine, worth a look)
04-mesh `stub peer: pair → online → wedged → offline (hysteresis)` —
stale service rows (`service-row[data-source-peer]`) sometimes 0 instead
of 3 after wedge. T3 #119 cached-services area. Passed on re-run.

## What remains (in Andrey's priority order)

1. **Telegram-style composer UX** (issue #134, task spec from Andrey's
   screenshots): `[paperclip][input][mic↔send]`; mic morphs to send arrow
   when input non-empty; press-and-HOLD mic = record (button scales ~3x,
   "liquid"), release-on-button = stop+attach+send, slide-off = lock
   recording (button becomes arrow; tap = stop+attach+send); terminal
   fallback toggle → floating bubble at top of chat; attachment chips →
   bubbles next to it, max 2 rows, overflow "and N more", removable.
   Same on desktop. ComposerActions.tsx is the starting point.
2. **Seamless peer-switch tokens** (issue #137): switching hosts in the
   web picker must NOT ask for the target's token — being authenticated
   to one mesh member authorizes all. Sketch: browser asks its daemon
   `GET /api/mesh/peers/{id}/web-token`; daemon relays a mint request
   over the signed mesh channel; browser stores per-host and switches.
   **Test this together between our two dev consoles.**
3. **QR "connect your phone"** (issue #138): button in Mesh tab → QR of
   the share link (tailscale IP + `#token=`). Pairs with #4.
4. **HTTPS via tailscale serve** (issue #135): getUserMedia is blocked on
   plain HTTP → voice physically can't work from the phone today (the
   button shows an honest error). Wire `tailscale serve` into
   web link / QR so links become `https://<host>.<tailnet>.ts.net/web/#…`.
5. **iOS keyboard gap** (part of #134): `interactive-widget` is
   Chrome-only; my `useKeyboardInset` (innerHeight − vv.height − offsetTop
   → root height calc) didn't fully fix Safari — composer still floats
   high with a void below. Likely needs the position:fixed +
   `top: vv.offsetTop` + `height: vv.height` pinning approach instead of
   height-only. Andrey's screenshot in chat history shows the failure mode.
   Also add `env(safe-area-inset-top)` padding in web — content overlaps
   the phone status bar.
6. **Folder-tree picker** (issue #136): web new-workspace currently
   window.prompt()s a raw path — replace with a browsable tree of the
   daemon machine (`GET /api/fs/dirs?path=` + create-folder) used by
   Sidebar new-workspace and agent-spawn cwd.
7. **Bidirectional close-propagation regression test** (small): close a
   tab in web → desktop updates via mux-state-changed, and vice versa.

## Gates before you push anything

```bash
./build.sh check && ./build.sh test     # rust: 1431 passing at handoff
pnpm exec tsc --noEmit && pnpm build
pnpm test:pw                            # or targeted suites; raw output:
node node_modules/@playwright/test/cli.js test -c tests/playwright --reporter=list
```

Never raw `codeterm` in bench/test scripts (prod-dispatch trap) — see
CLAUDE.md "Bench safety". Never kill port 7685.

## Issue train
#134 composer UX · #135 HTTPS/tailscale · #136 folder picker ·
#137 mesh web-tokens · #138 QR connect — all filed today, descriptions
carry the full specs.
