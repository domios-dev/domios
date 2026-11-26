# CodeTerm Changelog

All notable changes to CodeTerm. Format loosely follows [Keep a
Changelog](https://keepachangelog.com/), versioning is
[SemVer](https://semver.org/).

This file starts at the v1.0.1 → v1.1.0 cutover; pre-v1.0.1 history
lives in `git log` with no aggregated narrative.

## [1.23.3]

### Highlights

- Long chats scroll smoothly. Dragging the scrollbar, fast flicks, Home and End no longer show blank frames or jumps, End reaches the latest message, and older history loads automatically as you scroll up.
- Manage another machine from the sidebar: create and close workspaces, and close Teams and agents on the machine you are viewing over the mesh.
- OpenCode starts new users on a free model with a fresh model list. When an expired OpenCode Console key breaks the free model, Domios explains how to fix it.
- Agents mention tabs as clickable chips, including tabs on other machines.

### Changed

- A restoring tab shows Restoring in its header subtitle.
- General keeps a separate session for each provider, so switching providers and back resumes the earlier conversation.
- The Background Processes box shows one card per command and collapses its helper processes.
- Task history in Worker briefs is compact and collapsed in chat.
- Chat bubbles follow the theme instead of a machine's mesh color.

### Fixed

- Jumping to the latest message in a long chat no longer crashes the view.
- Browser surfaces and their Chrome pages close with the tab that owns them; startup removes any left behind.
- Mesh commands accept every peer id that Domios prints, reconnect after the mesh is turned off and on, and deliver remote Worker acknowledgements and reports to the Orchestrator that dispatched them.
- Settings changes and CLI updates apply to the remote machine you are viewing.
- Remote chat attachments open and send correctly.
- A General tab restored with a mismatched provider and session accepts messages again.
- Live model switches work on Claude terminal tabs, and the chat snapshot survives switching General to a Claude terminal session.
- Switching General's provider completes its install check without repeating onboarding.
- The Codex CLI version check no longer stays pending. On Windows, CLI version checks wait on process completion instead of polling.
- OpenCode without its CLI installed shows how to install it and offers a plain terminal.
- Dreams writes complete, verified diaries with links into the Memory graph.
- Cron runs report their real result instead of completing as failed.
- The tab header's Chat/Terminal switch is no longer collapsed.
- Numeric settings keep every step during rapid key presses, use the decimal point regardless of locale, and have accessible names.
- Continue-task text no longer truncates the task name in the composer header.
- Domios uses less CPU with many agent tabs: fewer WebView repaints and refetches, a smaller state payload, and faster plan reads.

### Known issues

- Browser clicks on an element whose role or name changed after the snapshot can target a stale element.
- On a discrete 800px mouse-wheel step, a long chat can flash its top edge for a single frame.

## [1.23.2]

### Highlights

- Applying a look sweeps the new theme across the whole interface. The reveal starts at the Apply button or, for chat commands, at the center of the chat. With reduced motion enabled, the theme fades in.
- The default team's Curator flags outstanding cleanup in each review round.
- Remove another machine from your account in Mesh Inventory or with `domios mesh remove`. If the machine is online, Domios signs it out first.

### Changed

- The tab header's Chat/Terminal switch uses the shared animated, icon-only segmented control.
- When you apply a look to the remote machine you're viewing, the new theme sweeps across the interface immediately. Changes to that machine's look also appear live.

### Fixed

- Mouse Back and Alt+Left no longer unload Domios and return you to the startup page. Thanks to [@zuev93](https://github.com/domios-dev/domios/pull/778) for the original fix.
- The sidebar highlights a single tab while you're viewing another machine. Switching machines selects that machine's last active tab, and returning restores yours.
- Claude background-process chips clear when completion or stop notifications arrive, even while the agent is idle or when one notification covers several processes.
- Claude opens Domios-owned data folders without a trust prompt. The approval applies to the resolved Domios folder.
- The machine switcher shows each machine's state below its name.

### Known issues

- A restored General tab with a mismatched provider and session can refuse incoming messages, including a look proposal forwarded from another machine.

## [1.23.1]

### Highlights

- Memory has a refreshed header and tab controls that adapt to narrow screens. Dreams shows each night's work as a constellation, with a detail view for the run and links into the Memory graph.
- General can propose a look for another machine paired to your account. You choose whether to apply it on that machine and can undo the change from the proposal card.

### Changed

- The Chat/Terminal toggle sits to the left of the tab name. The chat header hides the machine name when the mesh has only one machine.
- The selected sidebar tab no longer shows an accent bar, and the newest dream uses a New label without an accent stripe.
- Dream runs record the nodes they touched, the actions they took, the edges they created, and their current stage. The diary is linked to its run as soon as its draft is created.

### Fixed

- Routed messages stay in a single chat card when they arrive during an agent's turn.
- Restored chat bubbles with image attachments no longer show a large blank gap.
- Text attachments keep their original filenames.
- The queue chip is read-only in tabs where a manager or concierge controls input.
- Nightly REM runs on Windows without Bash or the Unix `date` command. Run timestamps use the host start time, and graph cleanup keeps removed edges from reappearing.
- Browser clicks that change the route within the same page now report completion. Browser actions promptly report blocking dialogs and leave them for the owner to handle. Named Browser profiles survive a restart.
- The Secrets dialog is available again from Settings, and the help points to the restored entry.
- Windows QA tools recognize the named pipe for an owned DEV tab while retaining their instance and caller checks.

### Known issues

- A restored General tab with a mismatched provider and session can refuse incoming messages, including a look proposal forwarded from another machine.

## [1.23.0]

### Highlights

- Appearance Lab "Try it": preview palettes, wallpapers, roundness, shadows, border brightness and text size live on the real app, then Apply or Discard. Every change shows exactly what it will look like, and Undo names the look it returns to.
- Chat wallpapers: a gallery of patterns, colours and fills, your own SVG or image, per-machine looks. Chats have no backdrop by default.
- Ask General for a look: it proposes a themed card with live contrast readings and Apply, and small tweaks ("make the background weaker") apply right away with Undo.
- Appearance changes play a smooth reveal over the real interface, without the text flickering.

### Changed

- Calmer interface: softer shadows, a quieter selected tab, even border brightness, a subtler focused composer, more air between sidebar rows.
- The machine menu shows each machine's live status and uses its peer theme for the entry colour.
- When the selected tab is scrolled out of the sidebar, a small "Name ↓" cue brings it back into view.
- File changes in chat show one row per file with the diff; raw tool data is under View raw.
- Background processes and sub-agents are labelled as such.
- The Updates button hides when everything is up to date.
- Try it opens in a side panel that overlays chat.
- Appearance Lab keeps contrast results near the colour controls and updates previews smoothly while you edit.
- Live agent activity appears in chat, and tool rows stay stable while work settles.
- macOS Login Items associates Domios background services with the app.
- Managed Payments explains when subscription plan changes take effect at renewal.
- Chat has a new top bar with a soft fade, the tab title, role and machine, and a compact Chat/Terminal toggle.
- The machine menu adds Add machine and Open Mesh, with an invitation when no other machines are paired.
- Press Ctrl+C or Cmd+C to copy selected chat messages.
- Queued messages appear in the composer context row.

### Fixed

- Closing a worker tab always works, even while its task is in review.
- A browser pane never shows another browser's page after one closes, and agent browsers close with their agent.
- Claude tabs with recent bound sessions resume automatically after a restart.
- Usage, @mentions, boards and images in a chat viewed from another machine come from that machine.
- Typing into rich web editors (for example X) through the browser tool registers the text.
- Settings Back and Analytics behave predictably; the macOS sidebar no longer jumps on hover.
- Lower server CPU under agent and mesh-viewer load.
- Remote shared tabs close like local tabs. Detach is gone, and only the owner with Full access can close a tab across machines.
- Restart works when a launch stalls.
- Team browser tabs restore without showing a false error.
- Neighbors no longer shows 'Neighbor gateway unavailable' after you toggle Neighborhood while signed out.
- Built-in themes switch from Settings without a contrast error, and light themes adapt the wallpaper for readability.
- The browser toolbar stays clear of the 'Take control' toast.
- New General sessions start on macOS without the 'General session configuration changed' error.
- Notification answers are delivered reliably, and notification history stays bounded.
- Cron runs without an attached session can complete and show the correct final status.

## [1.22.0]

### Highlights

- Live Transcript v2: a floating transcript panel with flowing passages, pause markers and a phone-size bottom sheet. It filters silence hallucinations in English and Russian, and forwards remarks addressed to an agent with a visible receipt.
- A local Concierge for Live Transcript can run on LM Studio or Ollama on this machine, with no hidden agent tabs. You can mark a wrong send and undo it, and those decisions are kept privately for training.

### Fixed

- Chat messages no longer stick or reappear after delivery during a running turn.
- The machine name in the title bar no longer overlaps its icons in narrow windows.
- The built-in browser reports the real screen width, so sites lay out for the actual window.
- Removed the "Try Live Transcript" bubble from the composer microphone; swipe-up lock and the tab menu entry remain.

### Known issues

- Local Concierge needs a running LM Studio or Ollama server; without one, Live Transcript uses the cloud Concierge.

## [1.21.1]

### Fixed

- Updates no longer fail when one tab's saved state exceeds 64 KiB (desk completion error).

## [1.17.3]

### Fixed

- Windows installer failed to run (exit 2) on Windows PowerShell 5.1; fixed.

## [1.17.2]

This update improves startup and session restoration, terminal input, mesh recovery, and update handling.

- Restore working tabs and OpenCode sessions more reliably, with clearer startup progress and fewer loading-state flashes.
- Improve streaming tool cards and terminal/chat continuity.
- Fix mesh admission and recovery. Machines removed from your mesh stop retrying and ask you to sign in again.
- While the plan cannot be verified, Domios keeps you unlocked and shows checking; paid features still require a verified plan.
- Improve updater recovery, platform packaging and signed macOS update archives.
- Include the DECRQSS terminal fix.

### Update notes and known limitations

- Updating from 1.17.1 may require closing tabs or using the installer.
- Adoption with 1.17.x peers stays on legacy links.
- The stray terminal "m" has not yet been attributed to a cause; the DECRQSS fix does not establish that it is resolved.
- The mimo provider key error is upstream.

## [1.17.1]

### Fixed

- Restore Enter submission in Terminal view on Windows and macOS by retaining the xterm input textarea after each Enter; disable inline predictive text on macOS. Fixes rollacode/codeterm-releases#99.
- Added update-restart cleanup regression tests. The main-thread cleanup fix already shipped in 1.17.0; this release adds coverage (Refs rollacode/codeterm-releases#98).

## [Unreleased]

### Added

- **`codeterm bridge migrate --to PEER`** — hot-relocate the active
  Telegram poller (and any future single-poller bridge) from the
  current host to a paired mesh peer without restarting either daemon.
  Orchestrates a six-step flow over `/api/bridge/{state,start,stop,announce}`
  + the existing mesh relay: source preflight → release source →
  enable + claim target via the target's own vault → verify both →
  optional `--announce` sentinel → auto-rollback on any failure.
  Exit codes: `0` success, `1` generic failure (rollback OK), `5`
  preflight (target unreachable / missing peer / no token + no vault),
  `6` usage error (running via `--mesh` is rejected — must run on the
  source machine), `7` split-brain (rollback also failed). Supports
  `--service` (default `telegram`), `--announce`,
  `--announce-chat-id` (default `186809246`), `--no-rollback`,
  `--force`, and `--claim-timeout-ms` (default `10000`). Each daemon
  resolves its own vault — secrets never cross the mesh, matching
  the existing `--mesh PEER mem secret …` local-only contract.
- **REST API: bridge runtime control.** Four new endpoints under
  `/api/bridge/*`:
  - `GET /api/bridge/state` — runtime + config snapshot
    (`active`, `config_enabled`, `token_present`,
    `has_secret_in_vault`, `started_at`, `last_poll_ok`,
    `last_poll_error`, `error_count`, `instance_name`).
    `has_secret_in_vault` uses `SecretStore::has` and never decrypts.
  - `POST /api/bridge/start` — `{service?, ensure_token?, wait_for_lock_ms?}`.
    With `ensure_token: true` the daemon pulls
    `telegram_bot_token` from the local vault when the token file is
    missing (412 if not in vault). Persists
    `integrations.telegram_enabled=true`, then waits for
    `last_poll_ok >= started_at` to confirm the upstream lock was
    claimed. 409 on Telegram conflict (another poller still active).
  - `POST /api/bridge/stop` — `{service?, timeout_ms?}`. Persists
    `telegram_enabled=false` **only after** the supervisor stop
    succeeds, so timeouts don't desync config from runtime.
    408 on timeout, idempotent when already idle.
  - `POST /api/bridge/announce` — `{service?, chat_id, text}`. One-shot
    outbound text via the existing channel; 503 if the bridge isn't
    active locally.
- **`BridgeSupervisor::op_guard()`** (tokio Mutex) serializes start /
  stop / boot-time spawn so concurrent operations on the same daemon
  never race.

### Changed

- **Docs (`docs/API.md`, `docs/MESH.md`, `docs/SKILLS.md`)** updated
  to cover the new REST endpoints and the migration orchestration
  including secret-locality guarantee and exit codes. Codeterm skill
  (`skills/codeterm/SKILL.md` + `references/telegram.md`) gained a
  `bridge migrate` quick-reference.

### Fixed

- **Team bootstrap no longer burns a turn on a readiness ping.** A freshly
  paired Planner read "say you are ready" as "tell the Orchestrator", wrote a
  pad file and sent it across, which forced the Orchestrator to wake, read and
  reply before the user had stated any goal. Both idle wake-up surfaces
  (`prompts/team_bootstrap_starter.md`, `prompts/planner_suffix.md`) now keep
  the acknowledgement inside the agent's own tab and name the counterpart
  message as reserved for a draft, decision, evidence or state change. The
  Planner surface prices every later message the same way, so a bare
  acknowledgement or restatement is not sent at all. Two prompt-surface
  regression tests guard the wake-up and the standing rule.

## [1.17.0] — 2026-10-28

Domios 1.17.0 gives each friend a receiving machine and a concierge whose
access follows the friendship's grants, with clearer conversation status
and controls throughout the app. Changes since v1.16.0:

### Neighbors

- **Choose where each friend is received.** Receive one friend on this
  machine from the sidebar, concierge row or conversation, with explicit
  takeover confirmation. Other friends can keep receiving on their current
  machines. The all-friends control identifies this machine and the number
  of friends affected. Moving receiving away retires that friend's local
  concierge and its delegated access.
- **Concierge access follows the friendship grant.** Scope, memory,
  workspace, file and machine-access permissions govern the concierge and
  its delegated agents. Applying permissions starts a fresh concierge
  session with the new instructions; the friend's conversation remains.
  Restoring an unchanged grant resumes the existing session.
- **General can edit the complete grant.** Partial changes preserve other
  fields, including memory enabled/disabled, allowed memory areas,
  workspace access, files, tab requests, interactive content, machine
  access and folder scope. Memory can be turned off explicitly.
- **Empty messages are refused before sending.** Empty or whitespace-only
  text returns `empty_body` without queueing a submission. File-only
  messages remain supported. Receiver-takeover and non-receiving-machine
  errors explain why the operation was refused.
- **Read receipts reflect actual reading or handling.** A visible incoming
  message or confirmed concierge handling advances the read state after
  delivery acknowledgement is committed. Delivery alone does not mark a
  message read; reconnects preserve newer read status.
- **See and stop concierge work.** Conversations show incoming handling
  status and provide Stop for the friend's active concierge. Replies to an
  owner-stopped request are refused before sending and are not offered as
  retryable submissions.
- **Flag suspicious incoming messages.** Owners and authorized agents can
  flag instructions, requests for private data, malicious links, spam or
  other concerns. Flags stay local and appear with the message; they are
  not sent to the friend or forwarded to bridges.
- **Choose concierge model, effort and speed** from the providers and
  options that support concierge work. The selected model appears below
  the concierge in the sidebar.

### Agents and interface

- **Codex Cloud is available in the new-session picker.** Environment
  discovery refreshes after sign-in and on request, so the picker can
  offer the available Cloud environments and execution modes.
- **The bundled control skill is now named `domios`.** Agent guidance and
  skill mirroring use the new name.
- **Consistent loading states** across the app, including terminal boot,
  with shared activity indicators and a Mesh card that keeps its size
  while loading.
- **Clearer session controls:** readable program names in command lines,
  and improved model and destination pickers. After a connection drops,
  the loaded app stays usable with a quiet reconnect pill while updates
  are paused; initial connection still has a loading state.
- **Mesh discovery improvements** include passive detection of macOS
  system-extension Tailscale and framed LocalAPI response handling.
- **Jobs is easier to navigate.** Import Blueprints from a file or
  repository through a dialog with a drop area; select General, a
  workspace or a Team with the shared destination picker. Jobs show as
  working only while a run executes, and an empty list has a clear empty
  state.

### Upgrade note

- **Upgrade both ends of a Mesh connection to 1.17.0.** The running app's
  version is the peer minimum, so a 1.17.0 installation refuses older
  peers. Official plugins keep their independently managed versions and
  minimum-host requirements; the app version bump does not bump plugins.

## [1.1.14] — 2026-05-05

Patch: stop the periodic console-window flash users were seeing on
Windows every ~15 seconds. Mesh discovery was shelling out to
`tailscale.exe ip -4` to learn its CGNAT IP, and each spawn briefly
attached a fresh `conhost` window before the GUI app got control back.

### Fixed

- **No more console flashing on Windows** — replaced the periodic
  `tailscale.exe` invocation in mesh discovery with a programmatic
  scan of local network interfaces (`if-addrs`) filtering for the
  Tailscale CGNAT range `100.64.0.0/10`. Same result, no subprocess,
  no flicker. The `/api/network` endpoint that exposed the same probe
  now reuses the helper.
- **Audited every other external-binary spawn** for the same
  conhost-flash failure mode and routed each through the existing
  `process_util::no_window_std` / `no_window_tokio` helper:
  `schtasks` (self-update + orchestrator), `reg` (PATH install),
  `where`/`which` (CLI probe), `opencode` (model list), and the
  `codeterm mem run-worker` subprocess.

## [1.1.1] — 2026-04-29

Patch: nightly memory dreamer was being marked Failed in cron history
even though the REM worker spawned successfully and produced
`daily_dream_<date>` mem items each night. Plus per-platform CI
plumbing on the develop branch.

### Fixed

- **Cron status race after explicit `cron complete`** — when an agent
  inside a cron tab called `codeterm cron complete`, the backend
  marked the run Success and closed the tab, but the
  `RunWatcherRegistry` watchdog kept its 30-second idle timer alive.
  After the tab closed there was no more output, the timer tripped,
  and `finish_run` overwrote the Success with Failed. `cron_complete`
  now calls `run_watcher.mark_completed_externally` on the tab's pty
  before `close_tab`, so the watchdog exits cleanly. Confirmed
  against the dreamer's 2026-04-25 → 2026-04-29 run logs.

### Added

- **Per-platform release workflows** — `.github/workflows/release.yml`
  was split into `release-windows.yml`, `release-macos.yml`, and
  `release-linux.yml`. Each builds independently on a tag push and can
  be suppressed via repo Variables `RELEASE_WINDOWS_ENABLED` /
  `RELEASE_MACOS_ENABLED` / `RELEASE_LINUX_ENABLED` (set to `false`).
  Secret rename `TAURI_PRIVATE_KEY` → `TAURI_SIGNING_PRIVATE_KEY`
  matches the actual repo secret.
- **Documentation review pass** — README translations carry
  TRANSLATION-TODO markers for the three sections that drifted from
  English (top-of-features GIFs, per-OS install split, "Canvas Tabls"
  rename). Stale references to deleted plans / endpoints in `docs/`,
  `.claude/agents/`, and prompt files are corrected.
- **Stronger orchestrator suffix** — `prompts/orchestrator_suffix.md`
  now mandates a final `codeterm agent report` call so spawning
  parents see completion immediately instead of polling status.

### Notes

- v1.1.1 ships **Windows-only** assets directly from a local BLACKSTAR
  build. The new per-platform workflows landed but went un-validated
  in CI because Actions free minutes were exhausted; revisit after the
  monthly quota resets. macOS DMGs are still produced manually from
  MacBook per `github-release` skill.

## [1.1.0] — 2026-04-29

The "resilience + features" push. Seven feature branches plus three
cleanup followups landed in `develop` on 2026-04-25; a pty cleanup fix
merged on 2026-04-26 and the wake-up auto-injection on 2026-04-29.
Design rationale for the 04-25 batch lives in the mem decisions chain
(`fact_design_decisions_chain_2026_04_25_codeterm_resilience_featur`).

### Added

- **General Agent wake-up auto-injection** — the L0 identity + L1
  critical-context payload from `codeterm mem wake-up` is now rendered
  directly into the GA system prompt under a new `## Live mem snapshot`
  section, so the agent boots already aware of identity, rules, peers,
  and critical facts without spending a turn on the CLI. Wake-up
  failures fall back to an empty section so GA bootstrap never blocks
  on mem. (4ea3e3f, merged in 4eb6088.)

- **Self-update via `tauri-plugin-updater`** — `codeterm update --check`,
  `--apply`, `--channel <stable|rc|nightly>`. Manifests are signed with
  ed25519 (public key embedded in `tauri.conf.json`). Auto-install runs
  the 9-step graceful shutdown (mem WAL checkpoint, supervisor stop,
  signed binary swap) and tabs auto-resume after restart. CI release
  workflow ships Win + macOS Intel + macOS ARM signed assets +
  channel-aware manifests. (Phases 1–4: 9215040, f1dfc7f, 5a6658b,
  fe8e738; merged in 540d42e.) See
  [`docs/contributing/auto-update-testing.md`](docs/contributing/auto-update-testing.md)
  and [`docs/plans/auto-update.md`](docs/plans/auto-update.md).

- **Agent restart CLI / API** — `codeterm agent restart --tab ID
  [--force] [--grace-secs N]` and `POST /api/tabs/{id}/agent/restart`.
  Reuses the existing `/exit` + auto-resume tab lifecycle (no new
  restart logic). Returns `202` with `session_was_stale` to signal
  whether the recorded session id was still valid; `429` rate-limits
  per-tab to avoid restart loops. (f7e130a, merged in d9577d8.)

- **Voice transcription in the Telegram bridge** — when
  `[integrations.voice_transcribe] enabled = true`, the bridge POSTs
  audio attachments to a local Whisper daemon (`daemon_url`, default
  `http://127.0.0.1:7891`) and substitutes the returned text inline
  before the message reaches any agent PTY. Covers Claude, Codex, and
  future agents with one upstream change instead of per-client hooks.
  (f994f48, merged in 6f99c38.) See
  [`docs/contributing/voice-transcription-testing.md`](docs/contributing/voice-transcription-testing.md).

- **Telegram HTML sanitizer** (`src-tauri/src/bridge/sanitize.rs`) —
  whitelists Bot-API HTML tags (`<b>`/`<strong>`, `<i>`/`<em>`, `<u>`,
  `<s>`, `<code>`, `<pre>`, `<a>`, `<tg-spoiler>`, `<blockquote>` and
  synonyms) and HTML-escapes everything else, including stray
  `<word>` substrings (e.g. `<date>` placeholders) that previously
  broke Telegram's parser. Long messages are split tag-aware so a
  `<code>` block can't be cut mid-tag. The General Agent prompt was
  updated to document the new whitelist. (f644a06, merged in 6b5a5ee.)

- **Mem subprocess supervisor with circuit breaker** — dreaming and
  librarian extraction now run as child processes of the daemon, gated
  by a circuit breaker (3 failures in 60s opens the circuit, manual
  reset). Worker death no longer takes the API down. Routed via
  `codeterm mem run-worker`. (0c94077, 4e29b75, eaa414e, merged in
  400778e.) See [`docs/RESILIENCE.md`](docs/RESILIENCE.md).

- **Mem heavy-ops semaphore + 503 timeout** — bounded concurrency on
  the heavy mem path; API connection-acquisition returns `503` after
  a short timeout instead of blocking indefinitely. Plus per-batch
  lock release in dreaming so API requests always win contention.
  (b54d5c8, 8a2d003, f67cf85; merged in 400778e.)

- **`codeterm agent reports` listing fix** — the parent-side reports
  CLI now correctly resolves the read-key against the writer's key
  (extracted as the free fn `resolve_report_target` with regression
  tests). Added `--limit N` and reverse-output ordering. (a5fe019,
  merged in 5505465.)

### Fixed

- **PTY child process termination on `PtyManager::kill`** — closing a
  tab now explicitly kills the child process instead of relying on
  shell-side cleanup, fixing leaked subprocesses when the user closed
  a tab mid-command. (be0d045, merged in 8227b5e.)

### Changed

- **`resolve_cli_path` resolution chain** — 5 steps:
  `CODETERM_CLI_PATH` env override → `data_dir/bin/codeterm` →
  `current_exe`-as-CLI → sibling lookup of `current_exe` → `PATH`.
  Replaces the 2-step heuristic that broke on Tauri MSI installs and
  macOS `.app` bundles. (24bd8a8.)

- **General Agent prompt** (`src-tauri/prompts/general_agent.md`) —
  new sections for self-update, agent restart, and voice transcription;
  Telegram-formatting section now documents the sanitizer.

### Internal cleanup

- Dropped dead in-process dreaming/librarian wrappers, gated the
  remaining helpers as `#[cfg(test)]` (5d3ca26).
- Annotated dreaming async test fixtures + the CLI SQL mirror so the
  sync/async split is visible at the call site (f1ea1f9).
- Test verifying in-process vs subprocess `history_log` parity
  (5d53208) — closes the cleanup followup #4 from the resilience plan.

### Documentation

- `docs/API.md` revision 30 → 31: documented agent restart endpoint
  + body, expanded the Updates section with channel endpoints +
  auto-install + manifest_url override semantics, and reorganised the
  cron paragraphs that had drifted under the wrong header.
- New contributor playbooks: `docs/contributing/voice-transcription-testing.md`,
  `docs/contributing/agent-restart-testing.md`,
  `docs/contributing/auto-update-testing.md`,
  `docs/contributing/auto-update-signing.md`.
- `docs/plans/auto-update.md` flagged as **shipped 2026-04-25**.
- README mirrored to all translation locales (auto-translated drafts)
  with the new agent restart, bridges, voice transcription,
  self-update, and supervisor sections. (1fb2bab, 12ea9a1.)
