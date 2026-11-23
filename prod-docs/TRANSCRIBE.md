# Voice transcription & local dictation

The composer supports local live dictation through a plugin that recognizes
speech as you record. The voice-note pipeline uses its configured clip
transcriber. Plugins manage their own engines and models.

## The two directions

| Direction | What it is | Owner |
|-----------|-----------|-------|
| **Inbound** | Telegram/Discord voice message → text before it reaches the GA | native |
| **Composer** | Local microphone → revisable text in the current draft | Live-capable Transcriber plugin |

The HTTP contract below describes a remote backend for recorded clips.

## The `/transcribe` contract

Any HTTP service that implements this can be the backend. Bring-your-own:
local whisper, a remote peer, cloud STT — Domios only cares about the contract.

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

Domios's bridge calls the daemon directly. Config:

```toml
[integrations.voice_transcribe]
enabled               = true
daemon_url            = "http://127.0.0.1:7891"
language              = ""        # "" = autodetect
timeout_ms            = 60000
fallback_raw_on_error = true      # on failure, GA still sees the raw file path
health_check_ttl_secs = 60
```

When enabled, a Telegram voice/video note is transcribed before the GA sees it.
The tab receives the attachment frame, then the speech in one element:

```
<domios type="attachment" kind="voice" path="<file>" label="voice" />
<transcript>…</transcript>
```

`&`, `<` and `>` inside the transcript are escaped, so spoken `</transcript>` or
`<domios` cannot close this element or forge a frame. The audio path stays on the
attachment. An untranscribed note does not emit `<transcript>`: `status="untranscribed"`
on the frame already records that outcome, and the following text explains the
failure. Notes already stored as bare text after the self-closing frame, and notes
using `-=-codeterm:voice file=<path>-=-`, stay readable. New transcribed notes always
use the `<transcript>` element. Point `daemon_url` at a live backend.

## Standing up the STT engine (reference: local whisper)

The reference backend is the `transcribe-telegram-voice` skill: a Python daemon
holding `faster-whisper large-v3` warm in VRAM.

- `GET /healthz`, `POST /transcribe` (all body forms above)
- Warm model → first-call latency ~25s (model download) then <1s per short clip
- **Watchdog**: evicts the model on external VRAM pressure (a game/CUDA job wants
  the GPU) or long idle; lazy reload on next request. VRAM pressure is the
  universal signal — no per-app hardcoding.
- Autostart via a logon-trigger scheduled task (pythonw, headless).

Find it: `domios mem search "transcribe telegram voice"` → the skill body has
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

## Live dictation in the composer

- Start from the composer microphone or search for **Dictation** with
  `@dictation`. Tap to toggle, hold to dictate until release, or slide up to lock.
- Review the recognized text in the draft. **Stop** finishes the current
  segment. **Cancel** discards its provisional text. Use **Send** to submit.
- If setup is required, choose **Set up with General** above the composer.
  General can guide installation of the official Transcriber plugin and its
  local Parakeet model while the draft remains in its own tab.
- In the plugin's glance panel, install, select, or test a model. The default
  Parakeet TDT 0.6B v3 supports Russian and English. Its model download is about
  487 MB and setup requires 64-bit Python 3.12.

## Today

Transcription uses an external service; today an external dictation driver can submit through the supported tab-send API. To add a new STT backend, implement `/transcribe` and point a URL at it. Don't put the model in the binary.

## Live Transcript: what it is (explain this to the user)

Live Transcript (Live Mode) keeps the microphone open and turns speech into text
locally with Parakeet STT, so nothing is uploaded for recognition. A hidden
Concierge, backed by a Scout model, reads the running transcript. It routes
actionable requests ("run the tests in the api tab", "ask the reviewer to look
at this") to the right agent and ignores chatter, thinking aloud and side
conversations. A floating Orb shows whether Live Mode is listening; collapse the
panel and it stays visible. The Thoughts drawer shows why each routing decision
was made. Open **Transcript** and press **Start Live Mode**, or swipe up on the
composer mic.

Scout tradeoffs: a local model (LM Studio or Ollama) is private and free per
request but needs disk and memory (Live Mode loads it automatically); a cloud
model (e.g. Groq) is lighter on the machine but costs more per request.

### From-scratch setup (General runs this)

1. Refresh and install the plugin: `domios plugin channel refresh codeterm-plugins`,
   then `domios plugin update transcriber` (or `domios plugin install transcriber
   --channel codeterm-plugins`). Version 0.5.1 or newer.
2. `domios plugin transcriber dictation-status`. If not ready, with the user's
   consent run `dictation-install parakeet-tdt-0.6b-v3-int8 --consent`, follow
   `dictation-progress`, then `dictation-select` and `dictation-test` the same id.
   The model is about 487 MB and needs 64-bit Python 3.12.
3. Scout: ask local or cloud. Warn about local model size, or that cloud costs more.
4. Check `GET /api/live-transcript/readiness`, then tell the user to open
   Transcript and press Start Live Mode. Do not touch voice-note settings.

## Live Transcript

Live Transcript retains committed speech for the tab that started capture. A
tab switch leaves its capture lease active. Explicit pause flushes the final
speech before recording the pause marker. Resume admits a new lease for the
client providing the microphone frames.

Run `domios live-transcript status` for transcriber setup, routing readiness and
the active binding revision. Use `overview` for the cursor and retained size
and `search` for paged owner history. The active consumer can `read`,
`mark-seen`, and `ack` using the current revision. Acknowledge only fully
processed sequences. Ordinary chat transcript commands keep their own group.
General can query `GET /api/live-transcript/readiness` for setup readiness
without obtaining tab history or bindings.

Initial Concierge Setup persists these preferences on the machine.

```toml
[live_transcript]
configured = false
provider = ""
model = ""
trigger = ""
default_mic_tap = false
```

`configured` records completion of setup. Empty provider and model values use
the available provider and its model defaults. `trigger` accepts
`scout_then_concierge`, `direct_batch`, `wake_word`, or `manual`. Scout sends
bounded request indicators to the configured Supporter model. If Scout is
unavailable, setup defaults to `direct_batch`. The microphone tap defaults to
off and can be saved independently of the router binding.

Readiness exposes these setup defaults and the eligible Concierge providers
with their available models. Status and overview include Concierge activity
and the most recent route error. A turn becomes stuck after 45 seconds and
reattaching the built-in Concierge replaces that epoch.

Use `--mode-only` to change the trigger in place.
The wake phrase is “Hey Domios”. An owner can use `--external-tab` to select a
tab with a separate cursor. The owner can inspect route events and the
corresponding authorized epoch. Raw history access follows the active binding.

### Live Transcript REST endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/live-transcript/readiness` | Setup readiness for ASR, Scout and Concierge (content-free; owner or General) |
| GET | `/api/live-transcript/{tab}/status` | Owner binding revision, capture lease and routing readiness |
| GET | `/api/live-transcript/{tab}/overview` | Owner cursor state and retained transcript bounds |
| GET | `/api/live-transcript/{tab}/history` | Paged committed owner transcript history |
| GET | `/api/live-transcript/{tab}/search` | Paged search over committed owner transcript |
| GET | `/api/live-transcript/{tab}/events` | Paged routing ledger events for the owner tab |
| GET | `/api/live-transcript/{tab}/epochs/{epoch}` | Scoped provider-published thinking and tool records for one exact epoch |
| GET | `/api/live-transcript/{tab}/training-log` | Owner routing training log summary and recent samples (`?limit=N`, default 50, max 200) |
| GET | `/api/live-transcript/{tab}/read` | Consumer read of committed speech at the active binding revision |
| POST | `/api/live-transcript/{tab}/attach` | Owner attach or update hidden router, external tab, or trigger mode |
| POST | `/api/live-transcript/{tab}/pause` | Owner explicit capture pause and speech flush |
| POST | `/api/live-transcript/{tab}/resume` | Owner resume capture admission |
| POST | `/api/live-transcript/{tab}/route` | Owner manual route of an exact sequence range |
| POST | `/api/live-transcript/{tab}/feedback` | Owner feedback verdict (`good_send` or `wrong_send`) on a routed forward (`{forwardId, verdict, note?}`) |
| POST | `/api/live-transcript/{tab}/ack` | Active consumer acknowledgement through a processed sequence |
| POST | `/api/live-transcript/{tab}/mark-seen` | Active consumer seen-cursor update at the current revision |
| POST | `/api/live-transcript/capture/start` | Direct secure owner start of a live microphone capture lease |
| POST | `/api/live-transcript/capture/{lease}/frame/{sequence}` | Stream one sequenced PCM capture frame for an active lease |
| POST | `/api/live-transcript/capture/{lease}/pause/{sequence}` | Flush and record an explicit owner pause marker on the lease |
| POST | `/api/live-transcript/capture/{lease}/cancel` | Cancel an active capture lease |

<!-- revision: 12 -->
