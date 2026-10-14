// Run: node scripts/run-ts-unit-tests.mjs src/components/Terminal/DelayedMessageModal.test.tsx
import assert from "node:assert/strict";
import { act } from "react";
import { Window } from "happy-dom";
import type { DelayedMessage } from "./delayedSend";
import type { DelayedMessagePatch, DelayedModalTarget } from "./DelayedMessageModal";

async function main() {
  const win = new Window({ url: "http://localhost/" });
  for (const key of ["window", "document", "navigator", "HTMLElement", "HTMLInputElement", "HTMLTextAreaElement", "HTMLButtonElement", "SVGElement", "Element", "Node", "Event", "MouseEvent", "KeyboardEvent"]) {
    Object.defineProperty(globalThis, key, { configurable: true, value: (win as unknown as Record<string, unknown>)[key] });
  }
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const [{ createRoot }, { DelayedMessageModal }] = await Promise.all([
    import("react-dom/client"),
    import("./DelayedMessageModal"),
  ]);

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const calls: { submit: DelayedMessagePatch[]; stash: string[]; deleted: number; sentNow: number; closed: number } = { submit: [], stash: [], deleted: 0, sentNow: 0, closed: 0 };
  const render = (target: DelayedModalTarget, extra: { edit?: boolean } = {}) => act(async () => {
    root.render(
      <DelayedMessageModal
        key={target.kind === "edit" ? target.message.id : "create"}
        target={target}
        busy={false}
        onClose={() => { calls.closed += 1; }}
        onSubmit={(patch) => calls.submit.push(patch)}
        onStash={extra.edit ? undefined : (text) => calls.stash.push(text)}
        onDelete={extra.edit ? () => { calls.deleted += 1; } : undefined}
        onSendNow={extra.edit ? () => { calls.sentNow += 1; } : undefined}
      />,
    );
  });
  const dialog = () => document.querySelector<HTMLElement>('[data-testid="delayed-message-modal"]');
  const button = (label: string) => Array.from(dialog()!.querySelectorAll<HTMLButtonElement>("button")).find((entry) => entry.textContent === label) ?? null;
  const minutesField = (label: string) => {
    const field = dialog()!.querySelector<HTMLInputElement>('[data-testid="delayed-minutes"]');
    assert.equal(field?.getAttribute("aria-label"), label);
    return field;
  };
  const modes = () => Array.from(dialog()!.querySelectorAll('[aria-label="When to send"] button')).map((entry) => entry.textContent);
  const setValue = async (element: HTMLInputElement | HTMLTextAreaElement, value: string) => {
    const setter = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(element), "value")!.set!;
    await act(async () => {
      setter.call(element, value);
      element.dispatchEvent(new Event("input", { bubbles: true }));
    });
  };

  await render({ kind: "create", text: "Re-run the flaky suite" });
  assert.equal(dialog()?.getAttribute("role"), "dialog", "the editor is a modal dialog");
  assert.equal(dialog()!.querySelector("textarea")?.value, "Re-run the flaky suite", "a new message starts from the draft text");
  assert.deepEqual(modes(), ["In", "At", "Every"], "a new message can be one-off or recurring");
  const before = Date.now();
  await setValue(minutesField("Send in, minutes")!, "12");
  await act(async () => { button("Schedule")!.click(); });
  assert.equal(calls.submit.length, 1, "Schedule submits once");
  assert.equal(calls.submit[0].text, "Re-run the flaky suite");
  assert.ok(calls.submit[0].scheduledAtMs >= before + 12 * 60_000 && calls.submit[0].scheduledAtMs <= Date.now() + 12 * 60_000, "In converts minutes to an absolute time");
  assert.equal(calls.submit[0].recurrence, undefined, "a one-off carries no recurrence");

  await act(async () => { button("Every")!.click(); });
  await setValue(minutesField("Repeat every, minutes")!, "30");
  await act(async () => { button("Schedule")!.click(); });
  assert.equal(calls.submit[1].recurrence?.everyMs, 30 * 60_000, "Every schedules a repeating message");

  await act(async () => { button("Every")!.click(); });
  assert.equal(dialog()!.querySelector('[aria-label="Quick times"]'), null, "quick times hide while repeating");
  await act(async () => { button("In")!.click(); });
  await act(async () => { button("Tomorrow")!.click(); });
  assert.equal(dialog()!.querySelector('[aria-label="When to send"] button[aria-pressed="true"]')?.textContent, "At", "a clock-time preset switches to At");

  await act(async () => { button("Stash")!.click(); });
  assert.deepEqual(calls.stash, ["Re-run the flaky suite"], "Stash keeps the text without a time");

  await setValue(dialog()!.querySelector("textarea")!, "   ");
  assert.equal(button("Schedule")!.disabled, true, "an empty message cannot be scheduled");

  const recurring: DelayedMessage = {
    id: "r1", tabId: "tab", text: "Sync reports", scheduledAtMs: Date.now() + 5 * 60_000, createdAtMs: 1, state: "queued",
    sessionChanged: false, recurrence: { everyMs: 10 * 60_000, untilMs: 999, count: 4 },
  };
  await render({ kind: "edit", message: recurring }, { edit: true });
  assert.deepEqual(modes(), ["Every"], "a recurring message keeps its rhythm");
  assert.equal(button("Stash"), null, "editing offers no stash");
  await setValue(minutesField("Repeat every, minutes")!, "20");
  await act(async () => { button("Save")!.click(); });
  assert.deepEqual(calls.submit[2].recurrence, { everyMs: 20 * 60_000, untilMs: 999, count: 4 }, "changing the interval keeps the series limits");

  await act(async () => { button("Send now")!.click(); });
  await act(async () => { button("Delete")!.click(); });
  assert.equal(calls.sentNow, 1, "Send now reaches its handler");
  assert.equal(calls.deleted, 1, "Delete reaches its handler");

  const stashed: DelayedMessage = { ...recurring, id: "s1", recurrence: null, state: "stashed", scheduledAtMs: 0 };
  await render({ kind: "edit", message: stashed }, { edit: true });
  assert.deepEqual(modes(), ["In", "At"], "a one-off edit chooses In or At");
  assert.equal(minutesField("Send in, minutes")?.value, "15", "a stashed message proposes fifteen minutes");

  await act(async () => { window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })); });
  assert.equal(calls.closed, 1, "Escape closes the editor");

  const system: DelayedMessage = {
    id: "system:heartbeat:pl:tab", tabId: "tab", text: "Plan reminder body", scheduledAtMs: 120_000,
    createdAtMs: 0, state: "queued", sessionChanged: false, origin: "system",
    reason: "sent because tasks task_a are still in progress in plan pl",
    recurrence: { everyMs: 60_000 },
    ticks: [{ atMs: 0, outcome: "sent", spinalVerdict: "processed", spinalReason: "delivered" }],
  };
  await render({ kind: "edit", message: system }, { edit: true });
  assert.equal(dialog()?.querySelector('[data-testid="heartbeat-origin"]')?.textContent, "system");
  assert.equal(dialog()?.querySelector('[data-testid="heartbeat-cadence"]')?.textContent, "every 1m");
  assert.match(dialog()?.querySelector('[data-testid="heartbeat-reason"]')?.textContent ?? "", /task_a/);
  assert.equal(dialog()?.querySelector('[data-testid="heartbeat-content"]')?.textContent, "Plan reminder body");
  const tick = dialog()?.querySelector('[data-testid="heartbeat-tick"]')?.textContent ?? "";
  assert.match(tick, /1970-01-01 00:00:00Z/);
  assert.match(tick, /sent/);
  assert.match(tick, /processed/);
  assert.match(tick, /delivered/);
  const labels = Array.from(dialog()!.querySelectorAll("button")).map((button) => button.textContent?.trim() || button.getAttribute("aria-label"));
  for (const forbidden of ["Delete", "Save", "Send now", "Cancel", "Schedule"]) {
    assert.equal(labels.includes(forbidden), false, `${forbidden} is absent`);
  }
  assert.ok(labels.includes("Close"));
  assert.equal(dialog()?.querySelector("textarea"), null, "a system heartbeat is not editable");

  await act(async () => { root.unmount(); });
  console.log("Delayed message modal passed");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
