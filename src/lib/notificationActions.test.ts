import assert from "node:assert/strict";
import { finish, it } from "../test/standaloneTest";
import { adoptStoredActions, neighborNotificationTarget, runNotificationAction } from "./notificationActions";
import { takeNeighborFocus } from "../components/Neighbors/neighborFocus";

it("stored prompt options retain their value and prompt identity for the answer route", () => {
  const identity = {
    kind: "directory_trust",
    options: [
      { label: "Trust and continue", value: "1" },
      { label: "Quit", value: "2" },
    ],
  };
  assert.deepEqual(
    adoptStoredActions([
      { type: "respond_prompt_option", label: "Trust and continue", value: "1", identity },
      { type: "respond_prompt_option", label: "Quit", value: "2", identity },
    ]),
    [
      { type: "respond_prompt_option", label: "Trust and continue", value: "1", identity },
      { type: "respond_prompt_option", label: "Quit", value: "2", identity },
    ],
  );
});

it("malformed prompt response actions are dropped", () => {
  assert.equal(
    adoptStoredActions([
      { type: "respond_prompt_option", label: "Trust", value: "1", identity: { kind: "directory_trust", options: [] } },
    ])?.length,
    0,
  );
});

it("Neighbor notification actions select the exact friend without opening an external window", async () => {
  const id = "a".repeat(64);
  assert.deepEqual(neighborNotificationTarget(`domios://neighbor/${id}`), { id, section: null });
  assert.equal(neighborNotificationTarget(`domios://neighbor/${id}/extra`), null);
  const prior = Object.getOwnPropertyDescriptor(globalThis, "window");
  const panels: string[] = [];
  Object.defineProperty(globalThis, "window", { configurable: true, value: { dispatchEvent: (event: Event) => { if (event instanceof CustomEvent && event.type === "codeterm:open-panel") panels.push(event.detail.panel); return true; }, open: () => { throw new Error("Unexpected external navigation"); } } });
  try {
    await runNotificationAction({ type: "url", label: "Open Neighbor", value: `domios://neighbor/${id}` });
    assert.deepEqual(panels, [`neighbor:${id}`]);
  } finally {
    if (prior) Object.defineProperty(globalThis, "window", prior);
    else Reflect.deleteProperty(globalThis, "window");
  }
});

it("Neighbor section links route only validated friend ids and fixed sections", async () => {
  const id = "b".repeat(64);
  const prior = Object.getOwnPropertyDescriptor(globalThis, "window");
  const events: string[] = [];
  Object.defineProperty(globalThis, "window", { configurable: true, value: {
    dispatchEvent: (event: Event) => { events.push(event.type); return true; },
    open: () => { throw new Error("Unexpected external navigation"); },
  } });
  try {
    for (const section of ["permissions", "concierge"] as const) {
      const url = `domios://neighbor/${id}/${section}`;
      assert.deepEqual(neighborNotificationTarget(url), { id, section });
      await runNotificationAction({ type: "url", label: "Review", value: url });
      assert.deepEqual(takeNeighborFocus(), { id, section });
    }
    assert.deepEqual(events, ["codeterm:neighbor-focus", "codeterm:open-panel", "codeterm:neighbor-focus", "codeterm:open-panel"]);
    for (const url of [
      `domios://neighbor/${id}/settings`, `domios://neighbor/${id}/permissions/extra`,
      `domios://neighbor/${id}/permissions?tab=foreign`, `domios://neighbor/${id}/concierge#other`,
      `domios://neighbor/${id}/permissions\n`,
      `domios://neighbor/${"b".repeat(63)}/permissions`, `domios://neighbor/${"B".repeat(64)}/permissions`,
      "domios://neighbor/../permissions",
    ]) {
      assert.equal(neighborNotificationTarget(url), null);
      await runNotificationAction({ type: "url", label: "Invalid", value: url });
      assert.equal(takeNeighborFocus(), null);
    }
    assert.equal(events.length, 4);
  } finally {
    if (prior) Object.defineProperty(globalThis, "window", prior);
    else Reflect.deleteProperty(globalThis, "window");
  }
});

finish();
