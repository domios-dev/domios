import { act, useMemo, useRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useMountedChatWindow } from "./useMountedChatWindow";
import { useChatScroll, type UseChatScrollResult } from "./useChatScroll";
import { clearPersistedChatScrollState } from "./chatViewport";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const SESSION = "window-wiring";
const VIEWPORT = 500;
let followTail = { current: true };
let host: HTMLElement;
let root: Root;
let scrollApi: UseChatScrollResult;
let mountApi: ReturnType<typeof useMountedChatWindow>;
type Row = { msg: { id: string; sourceMessageId: string }; height: number };
let rows: Row[];

function extent(element: HTMLElement): number {
  if (element.dataset.height) return Number(element.dataset.height);
  if (element.style.height) return Number.parseFloat(element.style.height) || 0;
  return Array.from(element.children).reduce((sum, child) => sum + extent(child as HTMLElement), 0);
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

function Harness({ active }: { active: boolean }) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  mountApi = useMountedChatWindow({ rows, scrollRef, active, followTailRef: followTail, sessionKey: SESSION });
  const contentVersion = useMemo(() => ({ rows, range: mountApi.range, revision: mountApi.revision }),
    [rows, mountApi.range.start, mountApi.range.end, mountApi.revision]);
  scrollApi = useChatScroll({ scrollRef, sessionKey: SESSION, active, loading: false,
    messageCount: rows.length, contentVersion, prependVersion: rows[0]?.msg.id, followTailRef: followTail });
  return <div data-testid="scroller" ref={(element) => {
    scrollRef.current = element;
    if (!element || Object.prototype.hasOwnProperty.call(element, "scrollHeight")) return;
    let top = 0;
    const max = () => Math.max(0, extent(element) - VIEWPORT);
    Object.defineProperties(element, {
      clientHeight: { configurable: true, get: () => VIEWPORT },
      scrollHeight: { configurable: true, get: () => extent(element) },
      scrollTop: { configurable: true, get: () => Math.min(top, max()), set: (value: number) => { top = Math.max(0, Math.min(value, max())); } },
    });
  }} onScroll={scrollApi.handleScroll}>
    <div>
      <div ref={mountApi.topSpacerRef} style={{ height: mountApi.range.top }} />
      {rows.slice(mountApi.range.start, mountApi.range.end).map(({ msg, height }) => <div key={msg.id} data-chat-window-row={msg.id} data-height={height}>
        <div data-message-id={msg.id} data-source-message-id={msg.sourceMessageId}>{msg.id}</div>
      </div>)}
      <div style={{ height: mountApi.range.bottom }} />
    </div>
  </div>;
}

function scroller(): HTMLDivElement { return host.querySelector('[data-testid="scroller"]')!; }
function mounted(): HTMLElement[] { return Array.from(host.querySelectorAll("[data-message-id]")); }
async function render(active = true) { await act(async () => root.render(<Harness active={active} />)); }
async function move(top: number) {
  await act(async () => { scroller().scrollTop = top; scroller().dispatchEvent(new Event("scroll", { bubbles: true })); });
}

beforeEach(() => {
  rows = Array.from({ length: 220 }, (_, index) => ({ msg: { id: `m${index}`, sourceMessageId: `s${index}` }, height: 100 }));
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
afterEach(async () => { await act(async () => root.unmount()); host.remove(); vi.restoreAllMocks(); clearPersistedChatScrollState(SESSION); });

describe("mounted chat wiring", () => {
  it("bounds a 220-row tail, follows streamed growth, and releases warm rows", async () => {
    await render();
    expect(mounted().length).toBeLessThanOrEqual(60);
    expect(mounted()[mounted().length - 1]?.dataset.messageId).toBe("m219");
    expect(scroller().scrollTop).toBe(scroller().scrollHeight - VIEWPORT);
    rows = rows.map((row, index) => index === 219 ? { ...row, height: 900 } : row);
    await render();
    expect(scroller().scrollTop).toBe(scroller().scrollHeight - VIEWPORT);
    await render(false);
    expect(mounted()).toHaveLength(0);
    await render();
    expect(mounted()[mounted().length - 1]?.dataset.messageId).toBe("m219");
    expect(scroller().scrollTop).toBe(scroller().scrollHeight - VIEWPORT);
  });
  it("mounts an exact reveal before scrolling and retains overlapping DOM rows", async () => {
    await render();
    expect(scrollApi.revealMessage("s40")).toBe(false);
    await act(async () => { expect(mountApi.prepareMessage("s40")).toBe(true); });
    await act(async () => { expect(scrollApi.revealMessage("s40")).toBe(true); });
    expect(followTail.current).toBe(false);
    const target = host.querySelector<HTMLElement>('[data-message-id="m40"]')!;
    expect(target.offsetTop - scroller().scrollTop).toBe(0);
    await move(scroller().scrollTop + 100);
    expect(host.querySelector('[data-message-id="m40"]')).toBe(target);
    expect(mounted().length).toBeLessThanOrEqual(60);
  });
  it("holds a detached anchor through row growth, prepend and warm return", async () => {
    await render();
    await move(10000);
    const anchor = mounted().find((row) => row.offsetTop >= scroller().scrollTop)!;
    const id = anchor.dataset.messageId!;
    const before = anchor.offsetTop - scroller().scrollTop;
    rows = rows.map((row) => row.msg.id === mounted()[0].dataset.messageId ? { ...row, height: 700 } : row);
    await render();
    expect(host.querySelector<HTMLElement>(`[data-message-id="${id}"]`)!.offsetTop - scroller().scrollTop).toBe(before);
    await act(async () => scrollApi.prepareForHistoryPrepend());
    rows = [{ msg: { id: "older", sourceMessageId: "older-source" }, height: 200 }, ...rows];
    await render();
    expect(host.querySelector<HTMLElement>(`[data-message-id="${id}"]`)!.offsetTop - scroller().scrollTop).toBe(before);
    await render(false);
    await render();
    expect(followTail.current).toBe(false);
    expect(host.querySelector<HTMLElement>(`[data-message-id="${id}"]`)!.offsetTop - scroller().scrollTop).toBe(before);
  });
});
