---
name: voice-testing
description: "Exercise CodeTerm voice transcription with the fake Whisper daemon and fallbacks."
---

# Voice-transcription testing playbook

How to exercise the bridge's voice-transcription path locally without
needing a real Whisper deployment. This covers the inbound side of the
Telegram bridge (and other bridge clients added later), where audio
attachments are POSTed to a local daemon and the returned text replaces
the `[voice]` line before the message reaches any agent PTY.

Cross-references:
- Backend code: `src-tauri/src/bridge/voice_transcribe.rs`
- Bridge integration: `src-tauri/src/bridge/manager.rs::build_message_body`
- Config schema: `src-tauri/src/config/schema.rs::VoiceTranscribeConfig`

## Why this lives upstream of the agent

Empirically the Claude Code `UserPromptSubmit` hook fires only on fresh
user prompts; mid-turn message injections (which is how Telegram
delivers most messages) bypass it. Codex has no equivalent hook. Doing
the transcription inside the bridge — before the formatted line is
written into the PTY — covers every current and future agent client
with one change.

## Config

```toml
[integrations.voice_transcribe]
enabled              = true
daemon_url           = "http://127.0.0.1:7891"
timeout_ms           = 60000
language             = ""           # empty = auto-detect
fallback_raw_on_error = true        # forward original [voice] line if daemon unreachable
health_check_ttl_secs = 60          # cache liveness probes for 60s to avoid flooding the daemon
```

`enabled = false` disables the feature entirely; the bridge forwards
the raw `[voice]` line as before, no daemon contact.

## Spinning up a fake daemon

The daemon contract is a single `POST /transcribe` endpoint that
accepts a multipart/form-data body with the audio file and returns
`{"text": "..."}`. Any HTTP server that mimics this shape works for
testing.

A minimal fake-Whisper that returns a canned response:

```python
# fake_whisper.py
from http.server import BaseHTTPRequestHandler, HTTPServer
import json

class H(BaseHTTPRequestHandler):
    def do_POST(self):
        if self.path == "/transcribe":
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps({"text": "hello from fake whisper"}).encode())
        else:
            self.send_response(404); self.end_headers()
    def do_GET(self):
        if self.path == "/health":
            self.send_response(200); self.end_headers(); self.wfile.write(b"ok")
        else:
            self.send_response(404); self.end_headers()
    def log_message(self, *a, **kw): pass

HTTPServer(("127.0.0.1", 7891), H).serve_forever()
```

```bash
python3 fake_whisper.py &
codeterm config set integrations.voice_transcribe.enabled true
codeterm bridge restart
```

Send any voice message to the bot — the line that lands in the
General Agent's Tab should read `[voice transcribed] hello from fake
whisper` instead of the raw `[voice]` placeholder.

## Health-check cache

`voice_transcribe::is_daemon_healthy` caches liveness for
`health_check_ttl_secs` to avoid hammering the daemon. To force a
re-check from a unit test or REPL, call
`voice_transcribe::reset_health_cache()` (the existing `bridge::manager`
tests do exactly this).

## Fallback behaviour

When the daemon is unreachable and `fallback_raw_on_error = true` (the
default), the bridge still forwards the message — the `[voice]` line
is left as-is so the conversation isn't lost. With
`fallback_raw_on_error = false`, the message is dropped from the PTY
and a warning is logged.

## Debugging

The bridge logs every transcription attempt:

```
[bridge] voice transcribe daemon=http://127.0.0.1:7891 elapsed_ms=412 chars=24
[bridge] voice transcribe failed: connect timeout (forwarding raw)
```

Tail the dev daemon log:

```bash
tail -f ~/.codeterm-dev/logs/codeterm.log | grep "bridge\] voice"
```
