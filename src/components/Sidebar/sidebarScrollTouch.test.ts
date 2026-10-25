import { afterEach, describe, expect, it, vi } from "vitest";
import { retainSidebarScrollAnchor } from "./sidebarScrollAnchor";
import { sidebarAnchorScrollTarget, sidebarScrollGesture, type SidebarScrollGesture } from "./sidebarScrollAnchorModel";

afterEach(() => { vi.unstubAllGlobals(); document.body.replaceChildren(); });

describe("sidebar scroll authority model", () => {
  const captured = { y: 120, scrollTop: 800, maximum: 1800 };
  it("selection and text/status changes cannot turn undelivered scrolling into a layout correction", () => {
    expect(sidebarAnchorScrollTarget(captured, { y: 85, scrollTop: 835, maximum: 1800 }, false)).toBeNull();
    expect(sidebarAnchorScrollTarget(captured, captured, false)).toBeNull();
  });
  it("retains real layout shifts and range clamps outside a native gesture", () => {
    expect(sidebarAnchorScrollTarget(captured, { ...captured, y: 180 }, false)).toBe(860);
    expect(sidebarAnchorScrollTarget(captured, { y: 220, scrollTop: 700, maximum: 700 }, false)).toBe(800);
  });
  it("withholds all corrections while native touch or momentum owns scrolling", () => {
    expect(sidebarAnchorScrollTarget(captured, { ...captured, y: 180 }, true)).toBeNull();
    expect(sidebarAnchorScrollTarget(captured, { y: 220, scrollTop: 700, maximum: 700 }, true)).toBeNull();
  });
  it("touch ownership lasts through release and momentum until scrollend, without a deadline", () => {
    let gesture: SidebarScrollGesture = { phase: "idle", moved: false };
    for (const event of ["touchstart", "touchmove", "touchend"] as const) gesture = sidebarScrollGesture(gesture, event);
    expect(gesture.phase).toBe("momentum");
    gesture = sidebarScrollGesture(gesture, "scrollend");
    expect(gesture.phase).toBe("idle");
  });
  it("a tap ends at release, but cancellation preserves native ownership", () => {
    const touch = sidebarScrollGesture({ phase: "idle", moved: false }, "touchstart");
    expect(sidebarScrollGesture(touch, "touchend").phase).toBe("idle");
    expect(sidebarScrollGesture(touch, "touchcancel").phase).toBe("momentum");
    expect(sidebarScrollGesture(touch, "scrollend").phase).toBe("touch");
    expect(sidebarScrollGesture({ phase: "momentum", moved: true }, "press").phase).toBe("idle");
  });
});

describe("sidebar scroll observer wiring", () => {
  it("selecting a row never writes scrollTop, even before the pending scroll event", () => {
    const fixture = scrollFixture();
    fixture.nativeScroll(35);
    fixture.select();
    expect(fixture.viewport.scrollTop).toBe(835);
    expect(fixture.writes).toEqual([]);
    fixture.dispose();
  });
  it("status/text churn never writes during touch, cancellation, or momentum, even when layout changes", () => {
    const fixture = scrollFixture();
    fixture.touch("touchstart");
    fixture.touch("touchmove");
    fixture.nativeScroll(35);
    fixture.churn(20);
    fixture.touch("touchend");
    fixture.nativeScroll(22);
    fixture.churn(15);
    fixture.touch("touchcancel");
    fixture.nativeScroll(13);
    fixture.churn(10);
    expect(fixture.viewport.scrollTop).toBe(870);
    expect(fixture.writes).toEqual([]);
    fixture.viewport.dispatchEvent(new Event("scrollend"));
    fixture.churn(30);
    expect(fixture.viewport.scrollTop).toBe(900);
    expect(fixture.writes).toEqual([900]);
    fixture.dispose();
  });
  it("cleanup removes gesture listeners and pending observer authority", () => {
    const fixture = scrollFixture();
    fixture.dispose();
    fixture.touch("touchstart");
    fixture.churn(60);
    expect(fixture.writes).toEqual([]);
  });
});

function scrollFixture() {
  let mutation: (() => void) | undefined;
  let resize: (() => void) | undefined;
  vi.stubGlobal("MutationObserver", class {
    constructor(callback: () => void) { mutation = callback; }
    observe() {}
    disconnect() { mutation = undefined; }
  });
  vi.stubGlobal("ResizeObserver", class {
    constructor(callback: () => void) { resize = callback; }
    observe() {}
    disconnect() { resize = undefined; }
  });
  const viewport = document.createElement("div");
  const body = document.createElement("div");
  const space = document.createElement("div");
  const row = document.createElement("div");
  row.className = "sidebar-tab";
  const button = document.createElement("button");
  button.textContent = "Open tab Worker";
  row.append(button);
  body.append(row);
  viewport.append(body, space);
  document.body.append(viewport);
  let scrollTop = 800;
  let rowTop = 920;
  const writes: number[] = [];
  Object.defineProperties(viewport, {
    clientHeight: { get: () => 768 }, offsetHeight: { get: () => 768 }, scrollHeight: { get: () => 2600 },
    scrollTop: { get: () => scrollTop, set: (next: number) => { writes.push(next); scrollTop = next; } },
  });
  viewport.getBoundingClientRect = () => new DOMRect(0, 0, 280, 768);
  row.getBoundingClientRect = () => new DOMRect(0, rowTop - scrollTop, 280, 44);
  row.getClientRects = () => Object.assign([row.getBoundingClientRect()], { item: () => row.getBoundingClientRect() });
  const dispose = retainSidebarScrollAnchor(viewport, body, space);
  button.onclick = () => { button.setAttribute("aria-current", "page"); row.append(document.createElement("span")); mutation?.(); };
  return {
    viewport, writes, dispose,
    nativeScroll: (delta: number) => { scrollTop += delta; },
    select: () => button.click(),
    touch: (type: string) => viewport.dispatchEvent(new Event(type, { bubbles: true })),
    churn: (delta: number) => { rowTop += delta; mutation?.(); resize?.(); },
  };
}
