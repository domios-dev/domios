import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { AgentRuntimeBanner } from "./AgentRuntimeBanner";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("opens the accessible runtime badge and wires Retry once, with outside and Escape dismissal", async () => {
  const retry = vi.fn();
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AgentRuntimeBanner state={{ state: "degraded", epoch: 1, reason: "opaque-fault" }} onRetry={retry} />));
    const badge = host.querySelector<HTMLButtonElement>('button[aria-haspopup="dialog"]')!;
    expect(badge).not.toBeNull();
    expect(badge.getAttribute("aria-label")).toBeTruthy();
    await act(async () => badge.click());
    expect(host.querySelector('[role="dialog"]')).not.toBeNull();
    const control = host.querySelector<HTMLButtonElement>('button[aria-label="Retry"]')!;
    expect(control).not.toBeNull();
    await act(async () => control.click());
    expect(retry).toHaveBeenCalledTimes(1);
    await act(async () => document.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true })));
    expect(host.querySelector('[role="dialog"]')).toBeNull();
    expect(document.activeElement).toBe(badge);
    await act(async () => badge.click());
    await act(async () => document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true })));
    expect(host.querySelector('[role="dialog"]')).toBeNull();
    await act(async () => badge.click());
    await act(async () => root.render(<AgentRuntimeBanner state={{ state: "loaded", epoch: 2 }} onRetry={retry} />));
    expect(host.childElementCount).toBe(0);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
