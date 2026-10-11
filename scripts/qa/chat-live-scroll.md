# Live Chat scroll regression check

Run on the Orchestrator's single reserved mobileqa DEV after its build health matches the merged SHA. This uses the original mobile QA driver's WebKit/iPhone configuration and fixture workspace MobileQA, Chat tab 91a91705. It sends real requests to the existing agent. No fixture tabs or agents are created.

```bash
CODETERM_INSTANCE=mobileqa CODETERM_API_PORT=7690 CHAT_QA_SHA=<merged-sha> \
  CHAT_QA_OUT=target-check/chat-after bash scripts/qa/chat-live-scroll.sh
```

The wrapper sources the dev-locked bench library, checks the exact tab with `dc`, and passes the verified connection to Node through `dev_run`. Never launch the Node script with inherited production connection variables.

The default matrix is desktop width 1280x900 and mobile 390x844, each with ten idle sends and ten streaming sends. Each streaming request gets an additional steering send after an assistant append while authoritative tab status is working. Missing streaming readiness fails the case. `CHAT_QA_WIDTH=390` selects only the mobile surface for a bounded reproduction.

The first idle send deliberately starts from history. Every send must render its optimistic bubble, its committed user echo, and its assistant reply in the already-open DOM. Mutation, resize, and scroll events record geometry at the next paint; snapshots also record before-send, optimistic, echo and reply phases. Every recorded sample after submission must have a bottom gap of at most one CSS pixel. Absolute scrollTop is allowed to change with the bottom as content height changes. Assistant completion requires an assistant row; the marker in the user's request cannot satisfy it.

A missing live turn fails after one bounded 60-second DOM deadline, saves the persisted transcript and a screenshot, and stops further sends into the blind view. A failure is a reproduction, never a matrix pass. Reload geometry is recorded after the matrix. Reports contain build identity, viewport, phase, stage geometry, tail row identity/visibility, and every failed case. Results are in the chosen output directory's report.json. Screenshots and transcripts stay in ignored target-check.

The focused unit checks cover deliberate upward detachment, retained content anchors, send reattachment, optimistic echo replacement, eviction, composer geometry clamping, resync geometry and stream growth:

```bash
npx vitest run src/components/Terminal/chatViewport.liveUpdates.test.ts \
  src/components/Terminal/useChatScroll.stability.test.tsx --maxWorkers=2
```

Before accepting, inspect the zero-failure report and confirm all forty matrix cases ran. A run stopping early or missing a streaming precondition is incomplete. Live resync during a send and physical iPhone keyboard/background behavior still need separate checks; this harness does not simulate Safari suspension.

After the forty send cases, eight arrival cases run: routed and delayed messages, each while pinned and detached, at both widths. Each starts a long streaming reply, verifies working status, and sends from the existing DEV Output tab 26de263e through its private pad. `CHAT_QA_SENDER` overrides that sender. Detached cases require the explicit newer-message affordance and preserve the first visible row's screen offset within one CSS pixel; then a deliberate latest action must reveal the real routed/reminder turn and assistant answer. Delayed arrival uses a one-second scheduled send, and readiness is established by the DOM event, never elapsed time. Inspect all forty-eight cases before acceptance.
