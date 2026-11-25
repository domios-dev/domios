import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { useDocumentPerfState } from "./useDocumentPerfState";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function Host() {
  useDocumentPerfState();
  return null;
}

afterEach(() => {
  vi.useRealTimers();
  delete document.documentElement.dataset.motion;
  delete document.documentElement.dataset.windowResizing;
  Object.defineProperty(document, "visibilityState", { configurable: true, value: "visible" });
});

it("pauses motion while hidden and flags window resizing until resize settles", async () => {
  vi.useFakeTimers();
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  try {
    await act(async () => root.render(<Host />));
    expect(document.documentElement.dataset.motion).toBeUndefined();
    expect(document.documentElement.dataset.windowResizing).toBeUndefined();

    Object.defineProperty(document, "visibilityState", { configurable: true, value: "hidden" });
    await act(async () => {
      document.dispatchEvent(new Event("visibilitychange"));
    });
    expect(document.documentElement.dataset.motion).toBe("paused");

    Object.defineProperty(document, "visibilityState", { configurable: true, value: "visible" });
    await act(async () => {
      document.dispatchEvent(new Event("visibilitychange"));
    });
    expect(document.documentElement.dataset.motion).toBeUndefined();

    await act(async () => {
      window.dispatchEvent(new Event("resize"));
    });
    expect(document.documentElement.dataset.windowResizing).toBe("true");

    await act(async () => {
      vi.advanceTimersByTime(200);
    });
    expect(document.documentElement.dataset.windowResizing).toBeUndefined();
  } finally {
    await act(async () => root.unmount());
    container.remove();
  }
});
