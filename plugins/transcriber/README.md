# Transcriber

Speech-to-text for CodeTerm — dictate into the focused pane instead of typing.

## What it does

- Provides the **transcriber** capability: audio in, text out.
- Backs onto a local [Whisper](https://github.com/ggerganov/whisper.cpp) server, or a mesh peer running one, so audio never leaves your machines.
- A glance view shows engine status (running / reachable).

## Requirements

- A `whisper-server` reachable at the configured address (defaults to `127.0.0.1:7891`), **or** a mesh peer exposing one.
- On macOS, `brew` is allowed so the plugin can offer to install/start a local engine; `curl` and `pkill` manage the server process.

## Permissions

| Permission | Why |
|------------|-----|
| `network: 127.0.0.1:7891` | Talks to the local Whisper server (or the mesh-forwarded peer engine). |
| `subprocess: whisper-server, brew, curl, pkill` | Start/stop and health-check the local engine; `brew` only for optional install. |
| `secrets` | Stores engine/peer connection details. |

## Use

Install the plugin, make sure a Whisper engine is reachable (the glance view tells you), then use the dictation control to transcribe into the active pane. Engine address and peer selection live in the plugin's settings.
