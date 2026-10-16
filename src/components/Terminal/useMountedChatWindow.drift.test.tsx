import { act, useRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useMountedChatWindow } from "./useMountedChatWindow";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const VIEWPORT = 300;
const rows = Array.from({ length: 60 }, (_, index) => ({ msg: { id: `m${index}` } }));
const heights = new Map(rows.map(({ msg }) => [msg.id, 100]));
let host: HTMLElement;
let root: Root;
let api: ReturnType<typeof useMountedChatWindow>;
let scroller: HTMLDivElement | null = null;
let scrollTop = 3000;
let writes = 0;
const userScrolling = { current: true };
const followTailRef = { current: false };

function extent(element: Element): number {
  const html = element as HTMLElement;
  if (html.dataset.chatWindowRow) return heights.get(html.dataset.chatWindowRow) ?? 0;
  if (html.style.height) return Number.parseFloat(html.style.height) || 0;
  return Array.from(element.children).reduce((sum, child) => sum + extent(child), 0);
}

function contentTop(element: Element): number {
  const parent = element.parentElement;
  if (!parent || element === scroller) return 0;
  let top = contentTop(parent);
  for (const sibling of Array.from(parent.children)) {
    if (sibling === element) break;
    top += extent(sibling);
  }
  return top + Number(element instanceof HTMLElement ? element.style.transform.match(/translateY\((-?[\d.]+)px\)/)?.[1] ?? 0 : 0);
}

function Harness() {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  api = useMountedChatWindow({ rows, scrollRef, active: true, followTailRef, sessionKey: "drift", userScrollingRef: userScrolling });
  return <div ref={(element) => {
    scrollRef.current = element;
    scroller = element;
    if (!element || Object.prototype.hasOwnProperty.call(element, "scrollTop")) return;
    Object.defineProperties(element, {
      clientHeight: { configurable: true, get: () => VIEWPORT },
      scrollHeight: { configurable: true, get: () => extent(element) },
      scrollTop: { configurable: true, get: () => scrollTop, set: (value: number) => { writes++; scrollTop = value; } },
    });
  }}>
    <div ref={api.topSpacerRef} data-testid="spacer" style={{ height: api.topSpacer }} />
    <div ref={api.rowsRef}>
    {rows.slice(api.range.start, api.range.end).map(({ msg }) => <div key={msg.id} data-chat-window-row={msg.id} />)}
    </div>
    <div style={{ height: api.range.bottom }} />
  </div>;
}

const spacer = () => host.querySelector<HTMLElement>('[data-testid="spacer"]')!;
const mountedRows = () => Array.from(host.querySelectorAll<HTMLElement>("[data-chat-window-row]"));

beforeEach(() => {
  scrollTop = 3000;
  writes = 0;
  userScrolling.current = true;
  for (const { msg } of rows) heights.set(msg.id, 100);
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    const top = this === scroller ? 0 : contentTop(this) - scrollTop;
    const height = this === scroller ? VIEWPORT : extent(this);
    return { top, bottom: top + height, height, left: 0, right: 0, width: 0, x: 0, y: top, toJSON: () => ({}) } as DOMRect;
  });
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

describe("useMountedChatWindow spacer drift", () => {
  it("holds a growing row beyond spacer exhaustion without touching native scrollTop", async () => {
    userScrolling.current = false;
    await act(async () => root.render(<Harness />));
    const [, second] = mountedRows();
    followTailRef.current = false;
    scrollTop = contentTop(second) + 10;
    await act(async () => root.render(<Harness />));
    const measured = mountedRows();
    const anchor = measured.find(row => row.getBoundingClientRect().top <= 0 && row.getBoundingClientRect().bottom > 0)!;
    const changed = measured[0];
    expect(changed).not.toBe(anchor);
    const anchorId = anchor.dataset.chatWindowRow;
    const before = anchor.getBoundingClientRect().top;
    const base = api.range.top;
    userScrolling.current = true;
    heights.set(changed.dataset.chatWindowRow!, 100 + base + 500);
    await act(async () => root.render(<Harness />));
    expect(writes).toBe(0);
    expect(Number.parseFloat(spacer().style.height)).toBe(0);
    const retained = host.querySelector<HTMLElement>(`[data-chat-window-row="${anchorId}"]`)!;
    expect(retained.getBoundingClientRect().top).toBe(before);
    userScrolling.current = false;
    const correction = api.foldDrift();
    scroller!.scrollTop += correction;
    expect(retained.getBoundingClientRect().top).toBe(before);
    expect(writes).toBe(1);
    expect(api.foldDrift()).toBe(0);
  });
  it("holds the viewport through a mid-gesture shift above it and releases it as a scroll delta", async () => {
    userScrolling.current = false;
    await act(async () => root.render(<Harness />));
    const [first, second] = mountedRows();
    followTailRef.current = false;
    scrollTop = contentTop(second) + 10;
    await act(async () => root.render(<Harness />));
    const base = api.range.top;
    expect(Number.parseFloat(spacer().style.height)).toBe(base);

    userScrolling.current = true;
    heights.set(first.dataset.chatWindowRow!, 40);
    await act(async () => root.render(<Harness />));
    expect(Number.parseFloat(spacer().style.height)).toBe(base + 60);

    expect(api.foldDrift()).toBe(-60);
    expect(Number.parseFloat(spacer().style.height)).toBe(base);
  });

  it("leaves the shift to the idle writer outside a user scroll", async () => {
    userScrolling.current = false;
    await act(async () => root.render(<Harness />));
    const [first, second] = mountedRows();
    followTailRef.current = false;
    scrollTop = contentTop(second) + 10;
    await act(async () => root.render(<Harness />));
    const base = api.range.top;
    heights.set(first.dataset.chatWindowRow!, 40);
    await act(async () => root.render(<Harness />));
    expect(Number.parseFloat(spacer().style.height)).toBe(base);
    expect(api.foldDrift()).toBe(0);
  });
});
