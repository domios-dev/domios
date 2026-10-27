import assert from "node:assert/strict";
import { finish, it } from "../test/standaloneTest";
import { adoptStoredActions, neighborNotificationTarget, runNotificationAction } from "./notificationActions";

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
  assert.equal(neighborNotificationTarget(`domios://neighbor/${id}`), id);
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

finish();
