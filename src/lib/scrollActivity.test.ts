import { afterEach, describe, expect, it, vi } from "vitest";
import { Window } from "happy-dom";
import { installScrollActivity } from "./scrollActivity";

describe("scroll activity", () => {
  afterEach(() => vi.useRealTimers());

  it("marks the scrolling element until it rests", () => {
    vi.useFakeTimers();
    const doc = new Window().document as unknown as Document;
    const list = doc.createElement("div");
    doc.body.appendChild(list);
    const uninstall = installScrollActivity(doc);

    list.dispatchEvent(new (doc.defaultView as unknown as typeof globalThis).Event("scroll"));
    expect(list.hasAttribute("data-scrolling")).toBe(true);
    vi.advanceTimersByTime(600);
    list.dispatchEvent(new (doc.defaultView as unknown as typeof globalThis).Event("scroll"));
    vi.advanceTimersByTime(600);
    expect(list.hasAttribute("data-scrolling")).toBe(true);
    vi.advanceTimersByTime(400);
    expect(list.hasAttribute("data-scrolling")).toBe(false);
    uninstall();
  });
});
