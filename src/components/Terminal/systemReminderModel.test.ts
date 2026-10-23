import assert from "node:assert/strict";
import test from "node:test";
import type { DelayedMessage, DeliveryTickView } from "./delayedSend";
import { composerDelayedModel } from "./composerDelayedModel";
import { canSendDelayedMessage } from "./delayedMessageModel";
import { isSystemDelayedMessage } from "./systemHeartbeatProjection";
import { nextEligibility, presentSystemReminder, systemReminderScope } from "./systemReminderModel";

const clock = (ms: number) => `@${ms}`;
const tick = (atMs: number, outcome: string, spinalReason: string): DeliveryTickView => ({ atMs, outcome, spinalVerdict: "skipped", spinalReason });

function heartbeat(id: string, patch: Partial<DelayedMessage> = {}): DelayedMessage {
  return {
    id, tabId: "tab", text: "", scheduledAtMs: 700_000, createdAtMs: 0, state: "queued", sessionChanged: false,
    origin: "system", recurrence: { everyMs: 600_000 }, reason: "reason-frag", ticks: [], ...patch,
  };
}

test("scope separates Worker and Orchestrator heartbeats", () => {
  assert.deepEqual(systemReminderScope("system:heartbeat:worker:tab"), { role: "worker", planId: null });
  assert.deepEqual(systemReminderScope("system:heartbeat:pl_a:tab"), { role: "orchestrator", planId: "pl_a" });
});

test("a next time is shown only when a scheduler tick anchors it", () => {
  const anchored = { scheduledAtMs: 700_000, recurrence: { everyMs: 600_000 }, ticks: [tick(100_000, "skipped", "not_due")] };
  assert.deepEqual(nextEligibility(anchored, 200_000), { kind: "at", atMs: 700_000 });
  assert.deepEqual(nextEligibility(anchored, 900_000), { kind: "due" });
  assert.deepEqual(nextEligibility({ ...anchored, ticks: [tick(150_000, "skipped", "not_due")] }, 200_000), { kind: "unknown" });
  assert.deepEqual(nextEligibility({ ...anchored, ticks: [] }, 200_000), { kind: "unknown" });
  assert.deepEqual(nextEligibility({ ...anchored, recurrence: null }, 200_000), { kind: "unknown" });
});

test("Worker reminder: System label, readable charter, actual why/next, no not-due rows", () => {
  const message = heartbeat("system:heartbeat:worker:tab", {
    text: '<codeterm-worker-charter task="task_q">\ncharter-frag\n</codeterm-worker-charter>',
    ticks: [tick(50_000, "skipped", "not_due"), tick(100_000, "sent", "delivered"), tick(400_000, "skipped", "not_due")],
  });
  const view = presentSystemReminder(message, 200_000, clock);
  assert.equal(view.label, "System");
  assert.equal(view.role, "worker");
  assert.equal(view.why, "reason-frag");
  assert.deepEqual(view.next, { kind: "at", atMs: 700_000 });
  assert.ok(view.meta.includes(clock(700_000)));
  assert.equal(view.lastSent, clock(100_000));
  assert.equal(view.lastHeld, null, "not-due checks are never surfaced");
  assert.deepEqual(view.sections.map((section) => section.body), ["charter-frag"]);
  for (const text of [view.meta, view.accessibleLabel, view.title]) assert.ok(!text.includes("<codeterm-"));
});

test("Orchestrator reminder keeps its plan, reports a held check and an unestablished time honestly", () => {
  const message = heartbeat("system:heartbeat:pl_a:tab", {
    text: '<codeterm-orchestrator-heartbeat plan="pl_a">\nstate-frag\n</codeterm-orchestrator-heartbeat>',
    ticks: [tick(30_000, "sent", "delivered"), tick(60_000, "suppressed", "healthy_snapshot")],
  });
  const view = presentSystemReminder(message, 200_000, clock);
  assert.equal(view.role, "orchestrator");
  assert.equal(view.planId, "pl_a");
  assert.deepEqual(view.next, { kind: "unknown" });
  assert.ok(!view.meta.includes(clock(700_000)) && !view.nextText.includes(clock(700_000)), "a placeholder time is never shown");
  assert.equal(view.lastSent, clock(30_000));
  assert.ok(view.lastHeld?.startsWith(clock(60_000)));
  assert.deepEqual(view.sections.map((section) => section.body), ["state-frag"]);
});

test("an ordinary user schedule stays a user delayed message", () => {
  const user = heartbeat("dmsg-1", { origin: "user", text: "user-frag", scheduledAtMs: 900_000, reason: null, ticks: undefined });
  const system = heartbeat("system:heartbeat:worker:tab");
  assert.equal(isSystemDelayedMessage(user), false);
  assert.equal(canSendDelayedMessage(user), true);
  assert.equal(canSendDelayedMessage(system), false);
  assert.deepEqual(composerDelayedModel([user, system], 0).chips.map((chip) => chip.id), ["dmsg-1"]);
});

test("plain reminder text passes through as one untitled section", () => {
  const view = presentSystemReminder(heartbeat("system:heartbeat:worker:tab", { text: " plain-frag " }), 0, clock);
  assert.deepEqual(view.sections, [{ kind: "", title: "", body: "plain-frag" }]);
});
