# Owner-style exploratory startup pass

The scripted matrix supplies timings and protocol evidence. This pass supplies
an agent's visual judgment of the real DEV window. Capturing a screenshot does
not complete a step: open the PNG with the agent's image viewer and describe
what is actually visible. Do not claim a visual pass from an accessibility tree,
an API response, or an unviewed file.

## Start and evidence

- [ ] Wait for the Orchestrator's READY on the requested fixed build. Keep the
  matrix and this pass held until then; do not restart DEV independently.
- [ ] Run the scripted matrix first, validate its contract, and report its first
  completed table immediately. Preserve failed health preflight rows. A failed
  matrix does not excuse omitting the visual pass when the window remains usable.
- [ ] Create `explore/` inside that run's results directory. Keep `findings.md`,
  `steps.jsonl`, screenshots, and exact API transcript/event evidence there.
- [ ] Resolve DEV through `evals/_lib/lib.sh`; all CLI actions use `dc`, and child
  processes use `dev_run`. Confirm build and port 7686 before acting.
- [ ] Reuse `chromium.connectOverCDP` and real WebView2 target selection from
  `driver.mjs`. Reuse its `tabButton`, `selectWorkspace`, and `select` approach,
  `core-journeys/lib/cdp.mjs` for MutationObserver pane waits, and
  `scripts/dev-screenshot.mjs` for window capture when appropriate. Do not open
  a replacement browser page or test the Vite page outside Tauri.
- [ ] Save an initial full-window screenshot and accessibility snapshot. Open
  the PNG yourself. Record the selected workspace/tab and expected build.
- [ ] Save the fresh mux/tab/workspace inventory. Make an ordered checklist of
  **every tab**, including shells and non-agent surfaces, with workspace, title,
  ID, provider/backing, frozen flag, last activity, and expected startup policy.
  Include General, recent tabs, untouched sleeping tabs, and every Endure fresh
  and used provider fixture. Reconcile visited IDs against this inventory at end.
- [ ] Preserve owner drafts, active tasks, and pending deliveries. In particular,
  carry aiderF's held send evidence without resending it or sending another probe
  over it. Record that unresolved delivery as a finding. Do not type PONG prose
  into shells or answer provider permission/login prompts automatically.

Each step gets a sequence number, action and time, tab/workspace ID, screenshot
path, whether the PNG was viewed, a concrete visual observation, and supporting
event/transcript paths. Use names such as `001-46137477-open.png`,
`002-46137477-submit.png`, and `003-46137477-reply.png`.

## Walk every tab, one at a time

- [ ] Select its workspace and expand any collapsed Team or sidebar section
  through the visible controls. Use fresh accessible roles/names, never CSS
  classes, force-clicks, or hidden controls. Capture and view the window after
  each action, including expansion and workspace selection.
- [ ] Click the tab's sidebar activation control and start the open timer.
  Capture the first rendered content and **view the PNG**. Record render time,
  selected tab identity, provider icon, visible status, and whether this matches
  the tab actually requested. Keep the frame even when the result is wrong.
- [ ] Judge the whole window: empty or missing content, lost used-session
  history, missing/wrong provider icons, raw Domios/HTML tags, overlapping or
  clipped controls, unusable scroll areas, and bare `Sleeping` text without a
  usable restore transition. A fresh fixture can legitimately have empty history;
  compare used fixtures with their saved API history before calling it lost.
- [ ] Watch loading and activity indicators against actual events. Record the
  1-second chat input and 5-second PTY-ready SLA breach when it occurs. If the
  state never resolves, wait on DOM/events under a 120-second failure deadline,
  capture and view that final state, then move on. Do not sleep to establish
  readiness or describe an unbounded spinner as a pass.
- [ ] For each awake agent without a pending send or owner draft, write one
  short real message: `Reply with the single word PONG. Do not run tools.` Add
  an opaque `explore:<run>:<tab>` probe token to distinguish this turn from the
  matrix's earlier PONG. This is one additional probe per agent for this pass.
- [ ] For chat, use that tab's visible composer immediately. Note time from
  visible content to text accepted, submit once, capture and view the window,
  and inspect sending/queued/refused feedback. Do not clear an owner's draft.
  For a verified PTY agent without a composer, send through `dc send` with
  `--require-agent`, using the prompt file as stdin; never paste prose into an
  unverified shell or a picker.
- [ ] Watch the reply appear **in the window** using DOM changes, capture it,
  and open that PNG yourself. Record submit-to-first-visible-reply time. Keep
  this distinct from the script's API first-reply and idle-completion times.
- [ ] Bind the visual reply to the exact new user turn and subsequent assistant
  PONG in this tab's own API transcript/session. Reuse `replyWindow` and the
  core-journeys reply oracle. A CLI receipt, status 200, or an old on-screen PONG
  does not prove this message landed. Retain failures and never replay an
  uncertain or held payload.
- [ ] For shells/surfaces, inspect their expected terminal or surface content
  and record the visual result; mark the agent-message step not applicable.

## Restore and navigation probes

- [ ] Choose an **untouched** sleeping tab from the saved inventory: the matrix
  may already have awakened its own fixtures. Capture and view its sidebar
  sleeping marker before opening, then click it once. Capture and view the
  immediate restore appearance, each meaningful transition, and the final ready
  or failed state. Record open-to-agent_ready from SSE and the owner-visible
  transition. Workspace selection can itself activate a tab: record that action
  as the wake origin rather than attributing its elapsed time to a later click.
- [ ] Choose one idle, unprotected, childless Endure fixture with a proven
  session and no pending send, draft, task, or generation. Record its session,
  backing and history. Use its **Tab actions** menu to close just that tab, then
  restore its exact entry from **Recently closed** through the visible Restore
  control. Discover its current accessible name from the snapshot. Never choose
  General, a Team root, or aiderF; never close a group or workspace for this probe.
- [ ] Capture and view the menu, disappearance, Recently closed entry, restore
  activation, and restored content. Prove the intended session, backing and
  history survived through API evidence as well as the visible screen. Record
  the old/new tab mapping instead of assuming an ID. Report a refusal or missing
  Restore control without substituting a newly spawned agent.
- [ ] Rapidly switch through General, a recent tab and a restored fixture, then
  back, using a short sequence of immediate accessible clicks with no settling
  sleeps. Capture and view after every click. Check selection/content identity,
  disappearing history, stale text/composer, layout jumps, icons and stuck loaders.
  Record click-to-content times. Do not send extra messages during this probe.
- [ ] Return the window to its original selected tab if it still exists. Reconcile
  visited IDs, screenshots viewed, messages attempted, and the restored fixture.
  List every omitted or inaccessible tab explicitly; do not imply full coverage.

## Findings and report

- [ ] Lead with what blocks the owner. Use **blocker** for inability to open,
  type, deliver, obtain a reply or use the app, including crashes; **bad** for
  materially broken restore/history/navigation or misleading status; **cosmetic**
  for a visible defect that leaves the journey usable. State the symptom and
  evidence rather than guessing a product root cause.
- [ ] For each finding, write: action taken; what was seen; actual timing and SLA
  when applicable; screenshot path and confirmation it was viewed; exact event,
  transcript or log evidence; severity; suspected area. Separate observed facts
  from the code-site hypothesis. Link the screenshot showing that actual tab.
- [ ] Write positive observations too: list which tab IDs visibly accepted the
  probe and displayed its new reply, with screenshot and transcript references.
- [ ] Report the findings list immediately after this pass. Include build,
  results directory, visited/total tab count, exceptions, scripted table link,
  and close/reopen and rapid-switch outcomes. Do not call the exploratory pass
  complete if screenshots were captured but not viewed.

Finding template:

```text
Severity: blocker | bad | cosmetic
Tab/workspace and build:
Action and timing:
Observed on screen:
Screenshot (viewed): explore/NNN-tab-action.png
API/event/log evidence:
Suspected area (hypothesis):
```

## Integrated event and no-loss round (local, on request)

Run this extension only when the owner requests an integrated live round. It
does not add a CI gate. Use the same build, evidence directory, DEV wrappers,
native window and screenshot-viewing rules as the checklist above. Reuse
`core-journeys/lib/oracle.mjs` and `replyWindow` for exact new own-session replies;
the standalone core-journeys runner requires a named disposable instance and
must not have its default-DEV refusal bypassed.

- [ ] After **every** DEV restart, complete a provider sweep before drawing
  conclusions from one provider: Claude, Grok, Aider, OpenCode, Codex Chat and
  plain shells, with at least one fresh and one used fixture of each. Inventory
  missing fixtures/providers explicitly; never substitute a Codex result for PTY
  proof. Preserve existing unresolved inputs and do not resend a prior probe.
  Add a per-provider table to the report with both fixture IDs, restored/fresh
  session provenance, boot-to-ready, immediate input acceptance, own-session
  reply evidence and cold/warm history results. Shells use unique-token echoes
  and real shell readiness/output. A missing start, slow ready, blocked input,
  absent own-session reply or lost used history fails that provider's row.

- [ ] Before measuring, require a successful focused repair gate when applicable,
  the exact committed build in health, and startup beyond service state
  construction without panic. A failed startup is a finding, not latency evidence.
- [ ] For every step append UTC start/end, monotonic elapsed milliseconds,
  workspace/tab/session/binding identity, action, screenshot path, `viewed`,
  visual observation and event/transcript references to `explore/steps.jsonl`.
  Retain pre-action state and failed trials. Write the full account and coverage
  table to `explore/report.md`, including the source SHA for each measured wave.
- [ ] Measure a used chat's first cold open and subsequent warm open separately:
  click-to-first-content, snapshot-ready, editable composer and text accepted.
  Record visible history and compare message IDs/session with saved API history.
  Do not infer input readiness from a loader disappearing. Chat acceptance must
  be within 1000 ms of visible content; PTY readiness within 5000 ms of boot/wake.
- [ ] Rapidly switch across each available provider and a shell, preserving
  drafts, and capture/view every first painted pane. Record click-to-content
  milliseconds, exact selected identity, history continuity and composer state.
  Missing content, a loader takeover, raw replay, flicker or a stale pane fails.
- [ ] After the individual walkthrough, run one bounded simultaneous-send wave
  across eligible idle agents without waiting for each reply before sending
  the next. Give every message a fresh run/tab/sequence token. Preserve drafts,
  active tasks and unresolved sends; list exclusions rather than overwriting them.
- [ ] Exercise immediate input before first readiness, addressing an untouched
  sleeping fixture, input while switching, and a short ordered burst in an
  isolated fixture. Record every attempted token and its held/written/delivered
  or failed state. Require one own-session user turn per token, ordered new
  replies, no duplicate bubbles and no replay after bind/restart. A written or
  uncertain payload is never resent. Capture unresolved and failed states.
- [ ] For each shell without active work, submit one harmless unique-token echo
  in its actual shell syntax and capture/view its output. Never send agent prose
  to a shell. Inspect non-agent surfaces through their intended visible controls.

Subscribe to native UI events and the DEV SSE stream before the following real
actions. Save before/action/after screenshots and actually view each one. An
event or HTTP response without its expected visible result does not pass. If a
capability, authenticated provider or safe fixture is absent, record the exact
uncovered case. Never inject a fake event as live proof.

| Surface | Real action and observable pass | Event evidence |
|---|---|---|
| Title | Rename one safe fixture, see its sidebar title change, then restore it | `tab-title-pinned` |
| Status/stream | Send a fresh prompt; see activity and new streamed assistant content in the correct tab | `tab-status-changed`, actual `tab-chat-<id>` / `tab-chat-msg-<id>` channels |
| Sidebar/restore | See General/recent tabs start and an untouched older tab retain its clock marker until opened/addressed | `mux-state-changed`, `tab-boot-stage` with exact tab and stage |
| Notifications | Through a DEV agent caller, create a kept test notification and exercise the offered action/resolution; see the corresponding bell entry and result | `notification-created`, and the exercised `notification-updated`, `notification-answered`, `notification-resolved` events |
| Recording | Start and stop a test capture through the DEV recording controls; see the active indicator and final stopped state | `local-recording-status`, `local-recording-capture` where applicable |
| Cron | Trigger one test job; see the kept run's real result and completion in Jobs | `cron-run-finished` and any observed run-specific event names |
| Persistence | Restart only DEV; see the same retained tab/session/history, notification and cron result | new listener/boot proof, retained IDs and restored UI |

- [ ] Report every missing event by its exact name, trigger, native versus SSE
  observation and visible consequence. Keep source wiring facts separate from
  executed behavior and hypotheses; absence from an unsubscribed channel is not
  evidence of an emitter regression.
- [ ] Reconcile the initial complete inventory with visited IDs, screenshots
  viewed, sends attempted and replies proven. Every omission, unsafe fixture,
  provider prompt, unavailable capability and unresolved message remains explicit.
  Include matrix contract validation and the close/restore outcome in the report.
