import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useChatActivationReconcile } from "./useChatActivationReconcile";

function Harness({ active, onReconcile }: { active: boolean; onReconcile: (cause: string) => void }) {
  useChatActivationReconcile(active, onReconcile);
  return null;
}

describe("useChatActivationReconcile", () => {
  let root: ReturnType<typeof createRoot> | null = null;
  let host: HTMLDivElement;

  afterEach(async () => {
    await act(async () => root?.unmount());
    host?.remove();
    vi.restoreAllMocks();
  });

  async function mount(active: boolean, onReconcile: (cause: string) => void) {
    Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true });
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    await act(async () => root!.render(<Harness active={active} onReconcile={onReconcile} />));
  }

  it("reconciles on window blur -> focus when active and stays silent when inactive", async () => {
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    const reconcile = vi.fn();

    await mount(true, reconcile);
    // Focus without a prior blur does not trigger a redundant reconcile.
    window.dispatchEvent(new Event("focus"));
    expect(reconcile).not.toHaveBeenCalled();

    window.dispatchEvent(new Event("blur"));
    window.dispatchEvent(new Event("focus"));
    expect(reconcile).toHaveBeenCalledTimes(1);
    expect(reconcile).toHaveBeenLastCalledWith("window_focused");

    // Repeated focus without another blur is ignored.
    window.dispatchEvent(new Event("focus"));
    expect(reconcile).toHaveBeenCalledTimes(1);

    // Returning from minimized deduplicates visibilitychange + focus into one catchup.
    let visibility: DocumentVisibilityState = "visible";
    Object.defineProperty(document, "visibilityState", { configurable: true, get: () => visibility });
    window.dispatchEvent(new Event("blur"));
    visibility = "hidden";
    document.dispatchEvent(new Event("visibilitychange"));
    visibility = "visible";
    document.dispatchEvent(new Event("visibilitychange"));
    window.dispatchEvent(new Event("focus"));
    expect(reconcile).toHaveBeenCalledTimes(2);
    expect(reconcile).toHaveBeenLastCalledWith("window_activated");

    // Inactive tabs do not reconcile on blur -> focus.
    reconcile.mockClear();
    await act(async () => root!.render(<Harness active={false} onReconcile={reconcile} />));
    window.dispatchEvent(new Event("blur"));
    window.dispatchEvent(new Event("focus"));
    expect(reconcile).not.toHaveBeenCalled();
  });
});
