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
