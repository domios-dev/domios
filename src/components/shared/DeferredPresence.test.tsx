import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import { DeferredPresence } from "./DeferredPresence";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("cancels an unrevealed loader and finishes an observed entry before exiting", async () => {
  const host = document.createElement("div");
  const root = createRoot(host);
  const render = (present: boolean) => act(async () => root.render(<DeferredPresence present={present}><section role="status" aria-label="fixture" /></DeferredPresence>));
  const animation = async (type: string, name: string) => act(async () => {
    const event = new Event(type, { bubbles: true });
    Object.defineProperty(event, "animationName", { value: name });
    host.firstElementChild!.dispatchEvent(event);
  });
  try {
    await render(true);
    await render(false);
    expect(host.querySelector('[role="status"]')).toBeNull();
    await render(true);
    await animation("animationstart", "ct-presence-enter");
    await render(false);
    expect(host.querySelector('[role="status"]')).not.toBeNull();
    await animation("animationend", "ct-presence-enter");
    expect(host.querySelector('[role="status"]')).not.toBeNull();
    await animation("animationend", "ct-presence-exit");
    expect(host.querySelector('[role="status"]')).toBeNull();
  } finally { await act(async () => root.unmount()); }
});
