import { act, useMemo, useRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useChatWindowView } from "./useChatWindowView";
import { clearTranscriptWindows, commitTranscriptBatch, useTranscriptWindow } from "./transcriptStore";
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
let prefixHeight = 0;
let bottomInset = 0;
let viewportHeight = VIEWPORT;
let loading = false;
let renders = 0;
let frames: Map<number, FrameRequestCallback>;

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
  renders += 1;
  const scrollRef = useRef<HTMLDivElement | null>(null);
  mountApi = useMountedChatWindow({ rows, scrollRef, active, followTailRef: followTail, sessionKey: SESSION });
  const contentVersion = useMemo(() => ({ rows, range: mountApi.range, revision: mountApi.revision }),
    [rows, mountApi.range.start, mountApi.range.end, mountApi.revision]);
  scrollApi = useChatScroll({ scrollRef, sessionKey: SESSION, active, loading,
    onViewportChange: mountApi.syncViewport, messageCount: rows.length, contentVersion, prependVersion: rows[0]?.msg.id, followTailRef: followTail });
  return <div data-testid="scroller" ref={(element) => {
    scrollRef.current = element;
    if (!element || Object.prototype.hasOwnProperty.call(element, "scrollHeight")) return;
    let top = 0;
    const max = () => Math.max(0, extent(element) - viewportHeight);
    Object.defineProperties(element, {
      clientHeight: { configurable: true, get: () => viewportHeight },
      scrollHeight: { configurable: true, get: () => extent(element) },
      scrollTop: { configurable: true, get: () => Math.min(top, max()), set: (value: number) => { top = Math.max(0, Math.min(value, max())); } },
    });
  }} onScroll={scrollApi.handleScroll}>
    <div>
      <div style={{ height: active ? prefixHeight : 0 }} />
      <div ref={mountApi.topSpacerRef} style={{ height: mountApi.range.top }} />
      {rows.slice(mountApi.range.start, mountApi.range.end).map(({ msg, height }) => <div key={msg.id} data-chat-window-row={msg.id} data-height={height}>
        <div data-message-id={msg.id} data-source-message-id={msg.sourceMessageId}>{msg.id}</div>
      </div>)}
      <div style={{ height: mountApi.range.bottom }} />
      <div style={{ height: bottomInset }} />
    </div>
  </div>;
}

let logicalApi: ReturnType<typeof useChatWindowView>;
function LogicalHarness() {
  const window = useTranscriptWindow(SESSION);
  logicalApi = useChatWindowView({ tabId: SESSION, windowKey: SESSION, transcript: window.transcript,
    anchorKey: window.anchorKey, followTailRef: followTail, loadOlderPage: async () => false });
  return null;
}
function scroller(): HTMLDivElement { return host.querySelector('[data-testid="scroller"]')!; }
function mounted(): HTMLElement[] { return Array.from(host.querySelectorAll("[data-message-id]")); }
async function render(active = true) { await act(async () => root.render(<Harness active={active} />)); }
async function move(top: number) {
  await act(async () => {
    scroller().scrollTop = top;
    scroller().dispatchEvent(new Event("scroll", { bubbles: true }));
    const callbacks = Array.from(frames.values());
    frames.clear();
    callbacks.forEach((callback) => callback(0));
  });
}

beforeEach(() => {
  rows = Array.from({ length: 220 }, (_, index) => ({ msg: { id: `m${index}`, sourceMessageId: `s${index}` }, height: 100 }));
  prefixHeight = 0;
  bottomInset = 0;
  viewportHeight = VIEWPORT;
  loading = false;
  frames = new Map();
  let nextFrame = 0;
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
afterEach(async () => { await act(async () => root.unmount()); host.remove(); vi.restoreAllMocks(); clearPersistedChatScrollState(SESSION); });

describe("mounted chat wiring", () => {
  it("coalesces native scroll events and skips renders within the same row window", async () => {
    await render();
    await move(scroller().scrollTop - 2990);
    frames.clear();
    const before = renders;
    await act(async () => {
      for (let index = 0; index < 8; index++) {
        scroller().scrollTop += 1;
        scroller().dispatchEvent(new Event("scroll", { bubbles: true }));
      }
    });
    expect(renders).toBe(before);
    expect(frames.size).toBe(1);
    await act(async () => {
      const callbacks = Array.from(frames.values());
      frames.clear();
      callbacks.forEach((callback) => callback(0));
    });
    expect(renders).toBe(before);
  });
  it("keeps initial mount and hydrated content pinned without reader events", async () => {
    rows = [];
    loading = true;
    await render();
    rows = Array.from({ length: 220 }, (_, index) => ({ msg: { id: `m${index}`, sourceMessageId: `s${index}` }, height: 100 }));
    await render();
    expect(followTail.current).toBe(true);
    loading = false;
    await render();
    expect(followTail.current).toBe(true);
    expect(scroller().scrollTop).toBe(scroller().scrollHeight - viewportHeight);
    await act(async () => root.unmount());
    root = createRoot(host);
    await render();
    expect(followTail.current).toBe(true);
    expect(scroller().scrollTop).toBe(scroller().scrollHeight - viewportHeight);
  });
  it("never classifies an unreported programmatic top reset as reader movement", async () => {
    await render();
    scroller().scrollTop = 0;
    await render();
    expect(followTail.current).toBe(true);
    await act(async () => scrollApi.handleContentLayout());
    await move(scroller().scrollTop);
    expect(followTail.current).toBe(true);
    expect(scrollApi.showScrollDown).toBe(false);
    expect(scroller().scrollTop).toBe(scroller().scrollHeight - viewportHeight);
  });
  it("repairs resize events after inset shrink reaches layout without a chat commit", async () => {
    await render();
    for (const height of [586, 650, 572, 592, 650]) {
      await act(async () => {
        window.dispatchEvent(new Event("resize"));
        viewportHeight = height;
        const callbacks = Array.from(frames.values());
        frames.clear();
        callbacks.forEach((callback) => callback(0));
      });
      expect(followTail.current).toBe(true);
      expect(scroller().scrollHeight - scroller().clientHeight - scroller().scrollTop).toBe(0);
    }
  });
  it("counts a delayed arrival against the committed boundary of an already detached reader", async () => {
    clearTranscriptWindows();
    const initial = Array.from({ length: 220 }, (_, i) => ({ id: `logical-${i}`, type: "assistant" as const, content: `row ${i}` }));
    commitTranscriptBatch(SESSION, { kind: "snapshot", authority: { sourceSessionId: "session", sessionBindingNonce: "nonce" },
      range: { start: 0, end: 22000 }, rows: initial });
    await act(async () => root.render(<LogicalHarness />));
    expect(logicalApi.newerMessageCount).toBe(0);
    const boundary = logicalApi.enrichedMessages[logicalApi.enrichedMessages.length - 1].msg.id;
    followTail.current = false;
    await act(async () => {
      commitTranscriptBatch(SESSION, { kind: "delta", authority: { sourceSessionId: "session", sessionBindingNonce: "nonce" },
        range: { start: 22000, end: 22200 }, rows: [
          { id: "delayed-user", type: "user", content: "reminder" },
          { id: "delayed-reply", type: "assistant", content: "reply" },
        ] });
      root.render(<LogicalHarness />);
    });
    expect(followTail.current).toBe(false);
    expect(logicalApi.newerMessageCount).toBe(2);
    expect(logicalApi.enrichedMessages[logicalApi.enrichedMessages.length - 1].msg.id).toBe(boundary);
    await act(async () => logicalApi.showLatest());
    expect(followTail.current).toBe(true);
    expect(logicalApi.newerMessageCount).toBe(0);
    expect(logicalApi.enrichedMessages[logicalApi.enrichedMessages.length - 1].msg.content).toBe("reply");
    await act(async () => root.unmount());
    root = createRoot(host);
    clearTranscriptWindows();
  });
  it("repairs inset-only and composer-height commits before observer delivery", async () => {
    await render();
    bottomInset = 42;
    await render();
    expect(scroller().scrollTop).toBe(scroller().scrollHeight - viewportHeight);
    viewportHeight = 422;
    await render();
    expect(scroller().scrollTop).toBe(scroller().scrollHeight - viewportHeight);
    expect(followTail.current).toBe(true);
  });
  it("retains detached mode and content through an extent clamp and delayed append", async () => {
    await render();
    await move(scroller().scrollTop - 300);
    const anchor = mounted().find((row) => row.offsetTop >= scroller().scrollTop) ?? mounted()[mounted().length - 1];
    const id = anchor.dataset.messageId!;
    const before = anchor.offsetTop - scroller().scrollTop;
    rows = rows.map((row) => row.msg.id === "m219" ? { ...row, height: 20 } : row);
    await render();
    expect(followTail.current).toBe(false);
    rows = [...rows, { msg: { id: "arrival", sourceMessageId: "reminder" }, height: 432 }];
    await render();
    expect(followTail.current).toBe(false);
    expect(host.querySelector<HTMLElement>(`[data-message-id="${id}"]`)!.offsetTop - scroller().scrollTop).toBe(before);
    expect(scrollApi.showScrollDown).toBe(true);
  });
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
  it("restores content identity after warm history changes and after complete remount", async () => {
    prefixHeight = 300;
    await render();
    await move(10000);
    const anchor = mounted().find((row) => row.offsetTop >= scroller().scrollTop)!;
    const id = anchor.dataset.messageId!;
    const before = anchor.offsetTop - scroller().scrollTop;
    await render(false);
    rows = rows.slice(8);
    await render(false);
    await render();
    expect(host.querySelector<HTMLElement>(`[data-message-id="${id}"]`)!.offsetTop - scroller().scrollTop).toBe(before);
    await act(async () => root.unmount());
    root = createRoot(host);
    followTail = { current: true };
    await render();
    expect(followTail.current).toBe(false);
    expect(host.querySelector<HTMLElement>(`[data-message-id="${id}"]`)!.offsetTop - scroller().scrollTop).toBe(before);
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
