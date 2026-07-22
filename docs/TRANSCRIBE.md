# Voice transcription & local dictation

How speech-to-text works in CodeTerm, and how you (an agent) stand up either
direction yourself. The guiding principle: **the STT engine is an external,
swappable service behind a `/transcribe` contract; CodeTerm only ships the thin
glue.** Never embed a whisper model in the CodeTerm binary — it's GPU/CUDA-bound,
platform-specific, and multi-GB. Keep it a daemon, point at it by URL.

## The two directions

| Direction | What it is | Owner |
|-----------|-----------|-------|
| **Inbound** | Telegram/Discord voice message → text before it reaches the GA | already native (`voice_transcribe.rs`) |
| **Outbound** | Local mic → text injected into a pane (dictation) | external driver today; native is planned |

Both speak the **same `/transcribe` contract**, so one STT daemon serves both.

## The `/transcribe` contract

Any HTTP service that implements this can be the backend. Bring-your-own:
local whisper, a remote peer, cloud STT — CodeTerm only cares about the contract.

```
GET  /healthz   -> {"ok": true, "model": "...", "device": "..."}
POST /transcribe
  body forms (accept all three for max compatibility):
    application/json     {"path": "C:/.../audio.oga", "language": "ru"|null}
    multipart/form-data  audio=<file>, optional language=ru
    audio/* | octet-stream   raw bytes; ?language=ru via query
  -> {"text": "...", "language": "ru", ...}   # only `text` is required
```

Path-JSON works when the daemon can see the file locally (same machine).
Multipart is the cross-machine form (the file is uploaded). A normalizing
proxy can accept one and forward the other — see "Proxy layer" below.

## Inbound: already wired

CodeTerm's bridge calls the daemon via `voice_transcribe.rs` (a thin client +
per-URL health cache). Config:

```toml
[integrations.voice_transcribe]
enabled               = true
daemon_url            = "http://127.0.0.1:7891"
language              = ""        # "" = autodetect
timeout_ms            = 60000
fallback_raw_on_error = true      # on failure, GA still sees the raw file path
health_check_ttl_secs = 60
```

When enabled, a Telegram voice/video note is transcribed before the GA sees it —
the pane gets text, not a `file://...oga` path. Nothing to build; just point
`daemon_url` at a live backend.

## Standing up the STT engine (reference: local whisper)

The reference backend is the `transcribe-telegram-voice` skill: a Python daemon
holding `faster-whisper large-v3` warm in VRAM.

- `GET /healthz`, `POST /transcribe` (all body forms above)
- Warm model → first-call latency ~25s (model download) then <1s per short clip
- **Watchdog**: evicts the model on external VRAM pressure (a game/CUDA job wants
  the GPU) or long idle; lazy reload on next request. VRAM pressure is the
  universal signal — no per-app hardcoding.
- Autostart via a logon-trigger scheduled task (pythonw, headless).

Find it: `codeterm mem search "transcribe telegram voice"` → the skill body has
the scripts (`daemon.py`, `transcribe.py`, `client.sh`, autostart installer).

## Proxy layer (optional, powerful)

Because `daemon_url` is just a seam, you can put a **proxy** there instead of the
engine. It pretends to be the transcriber (`/transcribe`) and routes underneath:
probe local, fall back to a remote peer, fan out to several backends, add
post-processing — and normalize the response back to `{text}` so the bridge never
notices.

Validated pattern (local-first proxy, ~50 LOC aiohttp): probe local whisper at
`:7891/healthz`; if up, forward path-JSON; if down, read the file from disk and
multipart-upload to a beefier peer (e.g. a 4090 box on the Tailnet). Same
response shape either path. Reusable for any peer that wants a remote GPU as
backup.

## Outbound: build a local dictation driver

Until native support lands, dictation is an external driver you run. Recipe:

1. **Hotkey** — global push-to-talk (e.g. Python `keyboard`, toggle on a combo).
2. **Capture** — record mic to a temp WAV (16 kHz mono; `sounddevice`).
3. **Transcribe** — POST the WAV (or its path, if the daemon is local) to
   `/transcribe`; read `text`. Collapse whisper's internal line breaks to spaces
   (`re.sub(r"\s+", " ", text)`) so auto-submit works.
4. **Inject into a pane** — two ways:
   - CLI: `codeterm send "<text>" --tab <id>` (auto-submits)
   - HTTP: `POST /api/panes/<pane_id>/send` with `Authorization: Bearer
     <service_token>` (register via `codeterm service add`, token is scoped to
     that endpoint). Good for a daemon with no shell.

### The focus caveat

Targeting "the pane the user is looking at" is **not reliable today**:
`codeterm tab current` is stale and `tab focus` doesn't update it (issue #123).
Workarounds for an external driver:
- Inject into a **fixed pane id** (`MIC_PANE`) — reliable now.
- Or skip pane targeting entirely and type into the **focused OS window** via
  synthesized keystrokes (`keyboard.write` + an `Enter`). Works in any app
  (terminal, Paint, browser) but is app-specific on submit and fights held
  hotkey modifiers — release ctrl/alt/shift before typing, and pause ~150 ms
  before the submit Enter.

A working prototype lives in the `transcribe-telegram-voice` skill scripts
(`mic-dictation.py` + `start-mic-dictation.ps1`).

## Where this is going (native)

Tracked in issues #123 (focus) and #124 (native pluggable voice input). Intended
end-state keeps the split — **engine external & swappable, glue native** — and
adds first-class UX handles CodeTerm alone can provide:

- **Trigger seam**: a native button/hotkey *dispatches outward* (it never
  transcribes). Either CodeTerm emits a `dictation.requested {pane_id}` event a
  driver subscribes to, or config declares a `start_cmd`/`driver_url` to invoke
  (mirrors `text_hooks`).
- **Live render**: stream partial transcripts into the focused pane's input
  buffer as you speak, instead of a one-shot paste at the end.
- **Lifecycle handles** for the driver: `begin(pane)` / `partial(text)` /
  `commit()` / `cancel()`, plus a real recording/transcribing **state
  indicator** (pane-border pulse / status-line glyph), not a tooltip.
- **Config** `[integrations.voice_input]`: `enabled`, `hotkey`, `device`,
  `language`, `daemon_url` (defaults to `voice_transcribe`), `auto_submit`,
  `target`.

### The right home: a plugin, not a bespoke integration

CodeTerm has a plugin system (`src-tauri/src/plugin/`, design in
`docs/contributing/architecture.md`): `plugin.toml` manifests, tiers
(`Native`/`Process`/`Wasm`), and a `PluginManager` whose `dispatch_event` routes
`SystemEvent`s to plugins by their declared `capabilities.hooks`. Dictation is a
natural **Process-tier plugin** — the plugin process owns mic capture + the
`/transcribe` client (no ML deps in core), and the host gives it handles:

- **Trigger** via `capabilities.hooks`: subscribe to a dictation-request event
  the native button/hotkey publishes (typed `SystemEvent` or the `Generic`
  escape hatch). `dispatch_event` already does the filtered routing.
- **Focus** without the stale `tab current` CLI: subscribe to
  `SystemEvent::PaneFocused { pane_id }` — the host already emits it, so the
  plugin always knows the focused pane (relevant to issue #123).
- **Settings** via `capabilities.settings`, **CLI** via `capabilities.commands`.
- **Live render + state indicator**: the one new host surface to add — a
  pane-input API (`begin/partial/commit/cancel`) the plugin streams partials into.

So: to add a new STT backend, implement `/transcribe` and point a URL at it.
To drive dictation today, run an external driver and inject via `codeterm send` or
the pane send API. To do it *right*, build it as a Process-tier plugin on the
existing plugin contract. Either way — don't put the model in the binary.
