import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import {
  DelayedSendQueuePanel,
  DelayedSendSchedulePanel,
} from "./DelayedSendPanel";
import type { DelayedMessage, RecurrenceRule } from "./delayedSend";

const windowInstance = new Window({ url: "http://localhost/delayed-send-panel-test" });
const windowAny = windowInstance as unknown as Window & typeof globalThis;
(globalThis as any).window = windowAny;
(globalThis as any).document = windowAny.document;
(globalThis as any).HTMLElement = windowAny.HTMLElement;
(globalThis as any).Node = windowAny.Node;
(globalThis as any).Event = windowAny.Event;
(globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;

let passed = 0;
let failed = 0;
function assert(condition: unknown, label: string) {
  if (condition) {
    passed++;
    console.log(`  ✓ ${label}`);
  } else {
    failed++;
    console.error(`  ✗ ${label}`);
  }
}

async function render(ui: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(ui));
  return { host, root };
}

async function click(host: HTMLElement, selector: string) {
  const button = host.querySelector<HTMLButtonElement>(selector);
  assert(button != null, `control exists: ${selector}`);
  if (button) await act(async () => button.click());
}

{
  let scheduledAt = 0;
  let recurrence: RecurrenceRule | undefined;
  const { host, root } = await render(
    <DelayedSendSchedulePanel
      busy={false}
      touchMode={false}
      onClose={() => {}}
      onConfirm={(at, rule) => { scheduledAt = at; recurrence = rule; }}
      onStash={() => {}}
    />,
  );
  assert(host.querySelector('[role="dialog"][aria-label="Schedule message"]') != null, "schedule dialog has a name");
  assert(host.querySelector('.ct-panel-card') != null, "schedule dialog uses the shared liquid panel");
  assert(host.querySelector('[role="tablist"][aria-label="Schedule type"]') != null, "schedule modes expose a named tablist");
  assert(host.querySelector(".ct-liquid-interactive") != null, "schedule actions use shared liquid interaction styling");
  for (const label of ["5m", "10m", "15m", "30m", "1h", "3h", "Tonight", "Tomorrow"]) {
    assert(
      host.querySelector(`[data-testid="delayed-send-preset"][data-preset-label="${label}"]`) != null,
      `one-shot preset exists: ${label}`,
    );
  }
  await click(host, '[data-testid="delayed-send-preset"][data-preset-label="10m"]');
  await click(host, '[data-testid="delayed-send-schedule-submit"]');
  assert(scheduledAt > Date.now(), "one-shot quick preset schedules a future fire");
  assert(recurrence === undefined, "one-shot schedule carries no recurrence rule");
  await act(async () => root.unmount());
}

{
  let scheduledAt = 0;
  let recurrence: RecurrenceRule | undefined;
  const { host, root } = await render(
    <DelayedSendSchedulePanel
      busy={false}
      touchMode={false}
      onClose={() => {}}
      onConfirm={(at, rule) => { scheduledAt = at; recurrence = rule; }}
      onStash={() => {}}
    />,
  );
  await click(host, '[role="tab"][aria-selected="false"]');
  await click(host, '[data-testid="delayed-send-interval-preset"][data-preset-label="5m"]');
  const stop = host.querySelector<HTMLSelectElement>('[data-testid="delayed-send-stop-mode"]');
  if (stop) {
    stop.value = "count";
    await act(async () => stop.dispatchEvent(new windowAny.Event("change", { bubbles: true })));
  }
  await click(host, '[data-testid="delayed-send-schedule-submit"]');
  assert(scheduledAt >= Date.now() + 5 * 60_000 - 1_000, "repeat preset schedules its first fire");
  assert(recurrence?.everyMs === 5 * 60_000 && recurrence.count === 3, "repeat mode carries interval and optional count stop");
  await act(async () => root.unmount());
}

{
  const now = Date.now();
  const message: DelayedMessage = {
    id: "series-1",
    tabId: "tab-1",
    text: "monitor",
    scheduledAtMs: now + 600_000,
    createdAtMs: now,
    state: "queued",
    sessionChanged: false,
    recurrence: { everyMs: 600_000, count: 3 },
    skippedTicks: 2,
  };
  let updated: { text?: string; scheduledAtMs?: number; recurrence?: RecurrenceRule } | undefined;
  let deleted = "";
  const { host, root } = await render(
    <DelayedSendQueuePanel
      messages={[message]}
      busyId={null}
      touchMode={false}
      onClose={() => {}}
      onDelete={(id) => { deleted = id; }}
      onSendNow={() => {}}
      onUpdate={(_id, patch) => { updated = patch; }}
    />,
  );
  assert((host.textContent ?? "").includes("repeats every 10m"), "queued series shows its interval");
  assert((host.textContent ?? "").includes("skipped 2"), "queued series shows skipped count");
  await click(host, 'button[aria-label="Edit"]');
  const interval = host.querySelector<HTMLInputElement>('[data-testid="delayed-send-edit-interval"]');
  if (interval) {
    const setter = Object.getOwnPropertyDescriptor(windowAny.HTMLInputElement.prototype, "value")?.set;
    setter?.call(interval, "15");
    await act(async () => interval.dispatchEvent(new windowAny.Event("input", { bubbles: true })));
  }
  await click(host, 'button[aria-label="Save"]');
  assert(updated?.recurrence?.everyMs === 900_000, "editing a series updates its interval");
  assert(updated?.recurrence?.count === 3, "editing interval preserves its stop condition");
  await click(host, 'button[aria-label="Stop recurring series"]');
  assert(deleted === "series-1", "stop control deletes the recurring series");
  await act(async () => root.unmount());
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
