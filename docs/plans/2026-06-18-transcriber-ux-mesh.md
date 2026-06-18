# Transcriber Self-Contained UX + Mesh Delegation — Plan

> Workers (Codex): one track per branch, STRICT TDD, commit BEFORE reporting,
> role hint inline (role skills 404). No backticks in shell strings. Report
> DONE/BLOCKED. The plugin IS the engine — no external scripts, no adapter, no
> LaunchAgent, no IPs.

**Goal:** Setting up voice transcription = pick a model in the plugin's panel →
**Install** → it downloads (with progress) and works. Change model → **Reinstall**
→ old model deleted, new one up. Optionally delegate to a mesh peer running the
same plugin — chosen from a dropdown, no IP. Zero agent improvisation, zero
hand-written adapters.

**Already in place (on plugins-rethink — NOT yet in the user's prod app):**
one-shot `whisper-cli` engine (the plugin IS the engine; no server/:7891/adapter),
`host.progress` download progress, orphan-model cleanup on switch (L4),
multilingual `small` default.

---

## P1 — self-contained model UX (PLUGIN ONLY, no host change). Branch `tx-model-ux` (codeterm-plugins)

**Role:** QuickJS/TS plugin engineer.

Model selection already exists in `transcriber/settings.schema.json` (the `model`
select). This track makes the glance/view panel a one-click installer around it.

- The glance view (`renderGlance`) shows: current model (from settings) + size +
  ready/idle/not-installed badge, and an **Install** button (when the active model
  isn't on disk) / **Reinstall** button (always, to re-pull or after a model
  change). `glanceAction`:
  - `install` / `reinstall` → ensure deps + **download the active model now**
    (pre-warm, not lazy-on-first-transcribe) with `host.progress` phases; on
    reinstall, the L4 orphan-prune removes the previous model; set
    `integrations.voice_transcribe` active so voice works immediately.
  - re-render glance with the new status.
- A model change in settings + Reinstall = clean swap (old deleted by L4, new
  downloaded). No server, no restart, no agent.
- Guard `typeof host.progress === "function"`. STRICT TDD against the fake host fs
  (install downloads the selected model + emits progress; reinstall after a model
  change prunes the old + downloads the new + keeps only active). Rebuild plugin.js.
- Gates: `npm run build:all && npm test && npx tsc --noEmit`.

## P1c — reshape the no-backend voice flow (small). Branch `tx-nobackend` (codeterm-alpha)

**Role:** Rust backend.

`src-tauri/src/bridge/voice_transcribe.rs` NoBackend note currently invites the
agent to "install then configure" (which caused the 20-step adapter mess). Change
it to point at the DETERMINISTIC path: "No speech-to-text engine. Run
`codeterm plugin install transcriber`, then open the Transcriber panel, pick a
model, and click Install — it downloads and enables itself. Do not build anything
by hand." Update the existing test. Gate: `cargo test voice_transcribe` + fmt.

---

## P2 — mesh delegation (NEEDS a new host capability — design track). Branches TBD

**Role:** Rust host + mesh engineer + plugin.

The plugin should let you pick a mesh machine and delegate transcription to the
same plugin there — no IP. Staged because it needs new host surface:

- **P2a — peer dropdown (feasible first):** a host capability `host.mesh.peers()`
  → list paired mesh peers (id + name). The transcriber settings/glance gains an
  "engine: this machine | <peer>" select populated from it. (Listing all peers is
  fine; detecting which peers have the plugin installed is a nice-to-have via a
  peer-snapshot query — fallback to listing all.)
- **P2b — remote transcribe:** a host capability to invoke a peer's transcriber
  over the existing mesh layer AND ship the audio file there (binary transfer over
  mesh), get text back. This is the heavy part — a new host mesh remote-invoke +
  file-transfer capability (audit against the confinement model per the
  architecture rules). Reuses CodeTerm's mesh pairing (no IP). 
- Designed separately (brainstorm → spec → pre-dispatch review) before any worker.

---

## Release (after P1 lands) — so the user can install + test

CRITICAL: the new transcriber UX depends on APP-side host capabilities that live
on `plugins-rethink` (e.g. `host.progress`), NOT in the user's released prod app.
So "release the plugin" = release the **app + plugin together**:
1. Land P1 (+ ideally the activity/lifecycle stack) on `plugins-rethink`.
2. Merge `plugins-rethink → develop`, run the app release pipeline
   (`.codeterm/skills/github-release`) → signed/notarized app the user installs.
3. Publish the transcriber to the `codeterm-plugins` channel (push main + the user
   does `codeterm plugin install transcriber` on the new app).
Fast alternative for testing-only: a local dev build (`./build.sh dev`) with the
plugin installed from the local dir — proves the UX without the full release.

## Verify (orchestrator)
- macOS: open Transcriber panel → pick small → Install → progress → record voice →
  Russian transcript. Switch to base → Reinstall → small deleted, base up. No
  agent, no adapter, no :7891.
- ROG: same (Windows zip-bootstrap path).
