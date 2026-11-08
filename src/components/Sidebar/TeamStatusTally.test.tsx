import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { TeamStatusTally } from "./TeamStatusTally";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const counts = { todo: 110, review: 3, blocked: 1, paused: 0, done: 286 };
let observers: Array<() => void> = [];
let fullWidth = 180;
let availableWidth = 200;

beforeEach(() => {
  observers = [];
  vi.stubGlobal("ResizeObserver", class {
    private readonly callback: () => void;
    constructor(callback: () => void) { this.callback = callback; }
    observe() { observers.push(this.callback); }
    disconnect() { observers = observers.filter((callback) => callback !== this.callback); }
    unobserve() {}
  });
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
    return { width: this.classList.contains("sidebar-team-status-fit") ? availableWidth : fullWidth } as DOMRect;
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

it("measures after layout, not during the commit, and fits before the first paint", async () => {
  const host = document.createElement("div");
  const root = createRoot(host);
  await act(async () => root.render(<TeamStatusTally counts={counts} />));
  expect(Element.prototype.getBoundingClientRect).not.toHaveBeenCalled();
  expect(observers).toHaveLength(1);
  const fit = host.querySelector(".sidebar-team-status-fit")!;

  fullWidth = 240;
  observers[0]();
  expect(fit.hasAttribute("data-compact")).toBe(true);

  availableWidth = 260;
  observers[0]();
  expect(fit.hasAttribute("data-compact")).toBe(false);
  await act(async () => root.unmount());
  expect(observers).toHaveLength(0);
});
