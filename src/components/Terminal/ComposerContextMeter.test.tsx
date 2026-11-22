import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, test, vi } from "vitest";
import { ComposerContextMeter } from "./ComposerContextMeter";
import { useClaimContext } from "../Usage/contextUsage";

const fixture = vi.hoisted(() => ({ claimed: false, context: { used_tokens: 20_000, window_tokens: 80_000 } }));
vi.mock("../../store/multiplexer/hooks", () => ({
  useMuxSelector: (select: (state: unknown) => unknown) => select({ tabs: { tab: { context: fixture.context } } }),
}));
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

function Claim() {
  useClaimContext("tab", fixture.claimed);
  return null;
}

test("the Context button reads the published window and returns focus on keyboard dismissal", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const render = () => act(async () => root.render(<><ComposerContextMeter tabId="tab" /><Claim /></>));
  try {
    await render();
    const trigger = host.querySelector<HTMLButtonElement>('button[aria-label="Context 25%"]')!;
    await act(async () => trigger.click());
    const dialog = document.querySelector('[role="dialog"][aria-label="Context window"]')!;
    expect(dialog.textContent).toContain(`${(20_000).toLocaleString()} of ${(80_000).toLocaleString()} tokens`);
    expect(dialog.textContent).toMatch(/compact older messages/);
    const close = dialog.querySelector("button")!;
    expect(document.activeElement).toBe(close);
    await act(async () => close.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true })));
    expect(document.querySelector('[aria-label="Context window"]')).toBeNull();
    expect(document.activeElement).toBe(trigger);
    await act(async () => trigger.click());
    await act(async () => document.body.dispatchEvent(new PointerEvent("pointerdown", { pointerType: "touch", bubbles: true })));
    expect(document.querySelector('[aria-label="Context window"]')).toBeNull();
    await act(async () => trigger.click());
    fixture.claimed = true;
    await render();
    expect(host.querySelector("button")).toBeNull();
    expect(document.querySelector('[aria-label="Context window"]')).toBeNull();
  } finally {
    fixture.claimed = false;
    await act(async () => root.unmount());
    host.remove();
  }
});
