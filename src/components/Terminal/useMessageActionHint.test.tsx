import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { messageHintBudget } from "./messageActionHint";
import { useMessageActionHint } from "./useMessageActionHint";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); });

it("wires still hover to the hint and suppresses it for reduced or paused motion", () => {
  vi.useFakeTimers();
  const claim = vi.spyOn(messageHintBudget, "claim").mockReturnValue(true);
  const opened = vi.spyOn(messageHintBudget, "opened").mockImplementation(() => {});
  let api: ReturnType<typeof useMessageActionHint>;
  const media = window.matchMedia("(prefers-reduced-motion: reduce)");
  let reduced = true;
  let coarse = false;
  vi.spyOn(window, "matchMedia").mockImplementation((query) => ({ ...media, matches: query === "(pointer: coarse)" ? coarse : reduced, addEventListener: vi.fn(), removeEventListener: vi.fn() }));
  function Harness() {
    const ref = useRef<HTMLDivElement>(null);
    api = useMessageActionHint(ref, false);
    return <div ref={ref}><output>{api.hint ? "hint" : "dot"}</output></div>;
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    act(() => root.render(<Harness />));
    act(() => { api.enter(); vi.advanceTimersByTime(600); });
    expect(host.textContent).toBe("dot");
    expect(claim).not.toHaveBeenCalled();
    reduced = false;
    coarse = true;
    act(() => { api.leave(); api.enter(); vi.advanceTimersByTime(600); });
    expect(claim).not.toHaveBeenCalled();
    coarse = false;
    host.dataset.motion = "paused";
    act(() => { api.leave(); api.enter(); vi.advanceTimersByTime(600); });
    expect(claim).not.toHaveBeenCalled();
    delete host.dataset.motion;
    act(() => { api.leave(); api.enter(); vi.advanceTimersByTime(599); });
    expect(host.textContent).toBe("dot");
    act(() => vi.advanceTimersByTime(1));
    expect(host.textContent).toBe("hint");
    act(() => { api.finish(); api.move(); vi.advanceTimersByTime(600); });
    expect(host.textContent).toBe("dot");
    expect(claim).toHaveBeenCalledTimes(1);
    act(() => api.opened());
    expect(opened).toHaveBeenCalledTimes(1);
  } finally {
    act(() => root.unmount());
    host.remove();
  }
});
