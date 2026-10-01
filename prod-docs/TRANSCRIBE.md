# Voice transcription & local dictation

How speech-to-text works in CodeTerm, and how you (an agent) stand up either
direction yourself. The guiding principle: **the STT engine is an external,
swappable service behind a `/transcribe` contract; CodeTerm only ships the thin
glue.** Never embed a whisper model in the CodeTerm binary — it's GPU/CUDA-bound,
platform-specific, and multi-GB. Keep it a daemon, point at it by URL.

## The two directions

| Direction | What it is | Owner |
|-----------|-----------|-------|
| **Inbound** | Telegram/Discord voice message → text before it reaches the GA | native |
| **Outbound** | Local mic → text injected into a tab (dictation) | external driver today; native is planned |

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

CodeTerm's bridge calls the daemon directly. Config:

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
the tab gets text, not a `file://...oga` path. Nothing to build; just point
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
4. **Inject into a tab** — two ways:
   - CLI: `codeterm send "<text>" --tab <id>` (auto-submits)
   - HTTP: `POST /api/tabs/<tab_id>/send` with `Authorization: Bearer
     <service_token>` (register via `codeterm service add`, token is scoped to
     that endpoint). Good for a daemon with no shell.

### The focus caveat

Targeting "the tab the user is looking at" is **not reliable today**:
`codeterm tab current` is stale and `tab focus` doesn't update it (issue #123).
An external driver should inject into a **fixed tab id** (`MIC_TAB`) through the
supported tab send API (`codeterm send` or `POST /api/tabs/{id}/send`).

A working prototype lives in the `transcribe-telegram-voice` skill scripts
(`mic-dictation.py` + `start-mic-dictation.ps1`).

## Today

Transcription uses an external service; today an external dictation driver can submit through the supported tab-send API. To add a new STT backend, implement `/transcribe` and point a URL at it. Don't put the model in the binary.

<!-- revision: 2 -->
