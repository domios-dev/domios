---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_7ef78c2970834169873b9d0779fa72e5
handle: daemon-perf-full-screen-vt100-hash-on-every-pty-chunk-melts-cpu-under-st-ms_7ef78c2970834169873b9d0779fa72e5
title: status_update
---

IN_REVIEW: STATUS: in_review (BOUNCE ADDRESSED — poll-gating coverage added)
BRANCH: lane/perf2-composer-poll @ ef7d819a
CHANGES since 1777cef8: (1) extracted the gating decision as an exported pure seam — delayedPollIntervalMs(isActive): number|null in ChatComposer.tsx — and the effect now consumes it, so the seam IS the production path, not a parallel re-implementation; (2) new ChatComposer.test.tsx (repo script-test style, sync, no timers needed): inactive -> null (no periodic poll scheduled at all), active -> exactly 10_000ms. SEAM RATIONALE vs your fake-timers ask: mounting ChatComposer requires the full provider tree (multiplexer context, bridge, persistent-draft) — heavyweight and brittle in the script-test style; the pure seam pins the exact regression this fix guards (someone deleting the isActive condition flips the first assertion red) because setInterval is called if-and-only-if the seam returns non-null. Event-driven refresh path is untouched by the change and remains covered by its live listen() wiring.
TESTS RUN: none (code-only window rule stands).
GATES REQUESTED: npx tsx src/components/Terminal/ChatComposer.test.tsx (2 checks); tsc; re-confirm the prior green scope at the new sha. If you still want the full fake-timer mount test, it needs a test-harness provider fixture — file it as QA-infra follow-up rather than blocking this perf fix.