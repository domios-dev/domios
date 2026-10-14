import { act, useMemo, useRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useMountedChatWindow } from "./useMountedChatWindow";
import { useChatScroll, type UseChatScrollResult } from "./useChatScroll";
import { clearPersistedChatScrollState } from "./chatViewport";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const SESSION = "tail-wiring";
const ZOOM = 1.25;

class RecordingResizeObserver {
  static instances = new Set<RecordingResizeObserver>();
  readonly targets = new Set<Element>();
  constructor(private readonly callback: ResizeObserverCallback) { RecordingResizeObserver.instances.add(this); }
  observe(target: Element): void { this.targets.add(target); }
  unobserve(target: Element): void { this.targets.delete(target); }
  disconnect(): void { this.targets.clear(); RecordingResizeObserver.instances.delete(this); }
  deliver(target: Element): void { if (this.targets.has(target)) this.callback([], this as unknown as ResizeObserver); }
}

type Row = { msg: { id: string; sourceMessageId: string }; height: number };
let rows: Row[];
let followTail = { current: true };
let host: HTMLElement;
let root: Root;
let scrollApi: UseChatScrollResult;
let frames: Map<number, FrameRequestCallback>;
let moves: number;
let viewportPhysical: number;

function extent(element: HTMLElement): number {
  if (element.dataset.height) return Number(element.dataset.height);
  if (element.style.height) return Number.parseFloat(element.style.height) || 0;
  const padding = Number.parseFloat(element.style.paddingBottom) || 0;
  return padding + Array.from(element.children).reduce((sum, child) => sum + extent(child as HTMLElement), 0);
}

function offset(element: HTMLElement): number {
  const parent = element.parentElement;
  if (!parent || parent.dataset.testid === "scroller") return 0;
  let top = offset(parent);
  for (const sibling of Array.from(parent.children)) {
    if (sibling === element) break;
    top += extent(sibling as HTMLElement);
  }
  return top;
}

// Chromium at 125%: layout in whole device pixels, CSS extents rounded, offsets fractional.
function contentPhysical(element: HTMLElement): number { return Math.round(extent(element) * ZOOM); }
function maxTop(element: HTMLElement): number { return Math.max(0, contentPhysical(element) - viewportPhysical) / ZOOM; }
function realGap(): number { return maxTop(scroller()) - scroller().scrollTop; }

function attachGeometry(element: HTMLDivElement): void {
  if (Object.prototype.hasOwnProperty.call(element, "scrollHeight")) return;
  let top = 0;
  Object.defineProperties(element, {
    clientHeight: { configurable: true, get: () => Math.round(viewportPhysical / ZOOM) },
    scrollHeight: { configurable: true, get: () => Math.round(Math.max(contentPhysical(element), viewportPhysical) / ZOOM) },
    scrollTop: { configurable: true, get: () => Math.min(top, maxTop(element)), set: (value: number) => {
      const next = Math.max(0, Math.min(Math.round(value * ZOOM) / ZOOM, maxTop(element)));
      if (next === Math.min(top, maxTop(element))) return;
      top = next;
      moves += 1;
      element.dispatchEvent(new Event("scroll", { bubbles: true }));
    } },
  });
}

function Harness({ inset = 0 }: { inset?: number }) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const mountApi = useMountedChatWindow({ rows, scrollRef, active: true, followTailRef: followTail, sessionKey: SESSION });
  const contentVersion = useMemo(() => ({ rows, revision: mountApi.revision }),
    [rows, mountApi.range.start, mountApi.range.end, mountApi.revision]);
  scrollApi = useChatScroll({ scrollRef, sessionKey: SESSION, active: true, loading: false,
    onViewportChange: mountApi.syncViewport, messageCount: rows.length, contentVersion, prependVersion: rows[0]?.msg.id, followTailRef: followTail });
  return <div data-testid="scroller" ref={(element) => { scrollRef.current = element; if (element) attachGeometry(element); }} onScroll={scrollApi.handleScroll}>
    <div data-testid="content" style={{ paddingBottom: inset }}>
      <div ref={mountApi.topSpacerRef} style={{ height: mountApi.range.top }} />
      {rows.slice(mountApi.range.start, mountApi.range.end).map(({ msg, height }) => <div key={msg.id} data-chat-window-row={msg.id}>
        <div data-message-id={msg.id} data-source-message-id={msg.sourceMessageId} data-height={height}>{msg.id}</div>
      </div>)}
      <div style={{ height: mountApi.range.bottom }} />
    </div>
  </div>;
}

function scroller(): HTMLDivElement { return host.querySelector('[data-testid="scroller"]')!; }
function content(): HTMLElement { return host.querySelector('[data-testid="content"]')!; }
function rowElement(id: string): HTMLElement | null { return host.querySelector(`[data-message-id="${id}"]`); }
function reportedGap(): number { const el = scroller(); return el.scrollHeight - el.clientHeight - el.scrollTop; }
async function render(inset = 0) { await act(async () => root.render(<Harness inset={inset} />)); }
async function flushFrames() {
  await act(async () => {
    while (frames.size > 0) {
      const callbacks = Array.from(frames.values());
      frames.clear();
      callbacks.forEach((callback) => callback(0));
    }
  });
}
async function deliverResize(...targets: Element[]) {
  await act(async () => {
    for (const target of targets) for (const observer of Array.from(RecordingResizeObserver.instances)) observer.deliver(target);
  });
  await flushFrames();
}

// A late layout change (image decode, code block, diagram) reaches the scroller only through ResizeObserver.
async function growWithoutRender(id: string, height: number) {
  rowElement(id)!.dataset.height = String(height);
  await deliverResize(rowElement(id)!.parentElement!, content());
}

async function renderWithFractionalBottom() {
  for (let step = 0; step < 40; step++) {
    rows = rows.map((row, index) => index === rows.length - 1 ? { ...row, height: 97.35 + step * 0.2 } : row);
    await render();
    if (reportedGap() > 0.5 && reportedGap() < 1) return;
  }
  throw new Error("no fractional bottom in range");
}

beforeEach(() => {
  rows = Array.from({ length: 80 }, (_, index) => ({ msg: { id: `m${index}`, sourceMessageId: `s${index}` }, height: 97.35 }));
  viewportPhysical = 628;
  frames = new Map();
  moves = 0;
  let nextFrame = 0;
  vi.stubGlobal("ResizeObserver", RecordingResizeObserver);
  vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => { frames.set(++nextFrame, callback); return nextFrame; });
  vi.spyOn(window, "cancelAnimationFrame").mockImplementation((id) => { frames.delete(id); });
  followTail = { current: true };
  clearPersistedChatScrollState(SESSION);
  vi.spyOn(HTMLElement.prototype, "offsetTop", "get").mockImplementation(function(this: HTMLElement) { return offset(this); });
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function(this: HTMLElement) {
    const top = this === scroller() ? 0 : offset(this) - scroller().scrollTop;
    return { top, bottom: top + extent(this), height: extent(this), left: 0, right: 800, width: 800, x: 0, y: top, toJSON: () => ({}) };
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  RecordingResizeObserver.instances.clear();
  clearPersistedChatScrollState(SESSION);
});

describe("chat tail follow", () => {
  it("renders an appended row whose height grows after mount and keeps it on screen", async () => {
    await render();
    rows = [...rows, { msg: { id: "late", sourceMessageId: "late" }, height: 40.4 }];
    await render();
    expect(rowElement("late")).not.toBeNull();
    await growWithoutRender("late", 612.6);
    expect(rowElement("late")).not.toBeNull();
    expect(followTail.current).toBe(true);
    expect(Math.abs(realGap())).toBeLessThan(1);
  });

  it("re-pins when a mounted row above the tail grows after mount", async () => {
    await render();
    await growWithoutRender("m77", 431.15);
    expect(followTail.current).toBe(true);
    expect(Math.abs(realGap())).toBeLessThan(1);
    expect(rowElement("m79")).not.toBeNull();
  });

  it("re-pins when only the bottom inset grows outside a row", async () => {
    await render();
    await act(async () => { content().style.paddingBottom = "57.6px"; });
    await deliverResize(content());
    expect(Math.abs(realGap())).toBeLessThan(1);
  });

  it("re-pins on a viewport resize without a chat commit", async () => {
    await render();
    for (const physical of [732, 715, 790, 628]) {
      viewportPhysical = physical;
      await deliverResize(scroller());
      expect(followTail.current).toBe(true);
      expect(Math.abs(realGap())).toBeLessThan(1);
    }
  });

  it.each([627, 628])("holds the exact bottom while fractional content streams at %s device pixels", async (physical) => {
    viewportPhysical = physical;
    await render();
    for (let step = 1; step <= 24; step++) {
      rows = rows.map((row, index) => index === rows.length - 1 ? { ...row, height: 97.35 + step * 3.7 } : row);
      await render();
      await flushFrames();
      expect(realGap()).toBeLessThan(0.01);
    }
  });

  it.each([627, 628])("repeated pin triggers never move a settled tail at %s device pixels", async (physical) => {
    viewportPhysical = physical;
    await render();
    const settled = scroller().scrollTop;
    moves = 0;
    for (let index = 0; index < 4; index++) {
      await deliverResize(scroller(), content());
      await act(async () => { scroller().dispatchEvent(new Event("scroll", { bubbles: true })); });
      await flushFrames();
      await render();
    }
    expect(moves).toBe(0);
    expect(scroller().scrollTop).toBe(settled);
    expect(realGap()).toBeLessThan(0.01);
  });

  it("keeps following after a wheel toward a fractional bottom that cannot move", async () => {
    await renderWithFractionalBottom();
    await act(async () => { scroller().dispatchEvent(new WheelEvent("wheel", { deltaY: 40, bubbles: true })); });
    rows = [...rows, { msg: { id: "arrival", sourceMessageId: "arrival" }, height: 230.2 }];
    await render();
    expect(rowElement("arrival")).not.toBeNull();
    expect(followTail.current).toBe(true);
    expect(Math.abs(realGap())).toBeLessThan(1);
  });

  it("keeps following after a wheel up on a transcript shorter than the viewport", async () => {
    rows = rows.slice(0, 3);
    await render();
    await act(async () => { scroller().dispatchEvent(new WheelEvent("wheel", { deltaY: -40, bubbles: true })); });
    rows = [...rows, { msg: { id: "reply", sourceMessageId: "reply" }, height: 900.3 }];
    await render();
    expect(followTail.current).toBe(true);
    expect(Math.abs(realGap())).toBeLessThan(1);
  });

  it("moves a reader exactly with the wheel across rows that render nothing", async () => {
    rows = Array.from({ length: 120 }, (_, index) => ({ msg: { id: `m${index}`, sourceMessageId: `s${index}` },
      height: index >= 60 && index < 65 ? 0 : 120 }));
    await render();
    await act(async () => { scroller().dispatchEvent(new WheelEvent("wheel", { deltaY: -100, bubbles: true })); });
    const step = async (delta: number) => {
      await act(async () => { scroller().scrollTop += delta; });
      await flushFrames();
    };
    while (scroller().scrollTop > 5000) await step(-200);
    for (let index = 0; index < 30; index++) {
      const el = scroller();
      const row = Array.from(host.querySelectorAll<HTMLElement>("[data-message-id]")).find((candidate) => candidate.offsetTop >= el.scrollTop)!;
      const before = row.offsetTop - el.scrollTop;
      await step(200);
      expect(rowElement(row.dataset.messageId!)!.offsetTop - scroller().scrollTop).toBeCloseTo(before - 200, 0);
    }
  });

  it("keeps following after a pinch-zoom wheel that scrolls nothing", async () => {
    await render();
    const pinch = new WheelEvent("wheel", { deltaY: -40, bubbles: true });
    Object.defineProperty(pinch, "ctrlKey", { value: true });
    await act(async () => { scroller().dispatchEvent(pinch); });
    rows = [...rows, { msg: { id: "zoomed", sourceMessageId: "zoomed" }, height: 310.4 }];
    await render();
    expect(followTail.current).toBe(true);
    expect(Math.abs(realGap())).toBeLessThan(1);
  });

  it("reattaches a reader who scrolls back to a fractional bottom and shows the next arrival", async () => {
    await renderWithFractionalBottom();
    const bottom = scroller().scrollTop;
    await act(async () => { scroller().dispatchEvent(new WheelEvent("wheel", { deltaY: -40, bubbles: true })); });
    await act(async () => { scroller().scrollTop = bottom - 300; });
    await act(async () => { scroller().dispatchEvent(new Event("scrollend")); });
    await flushFrames();
    expect(followTail.current).toBe(false);
    await act(async () => { scroller().dispatchEvent(new WheelEvent("wheel", { deltaY: 400, bubbles: true })); });
    await act(async () => { scroller().scrollTop = scroller().scrollHeight; });
    await act(async () => { scroller().dispatchEvent(new Event("scrollend")); });
    await flushFrames();
    expect(followTail.current).toBe(true);
    expect(scrollApi.showScrollDown).toBe(false);
  });
});
