import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { conversationEntries, revealNewerMessages } from "./chatPagingModel";
import { clearPersistedChatScrollState } from "./chatViewport";
import { chatWindowProjection, timelineItemKey } from "./chatWindow";
import { useChatScroll, type UseChatScrollResult } from "./useChatScroll";

const journal = vi.hoisted(() => vi.fn());
vi.mock("../../lib/clientJournal", () => ({ recordClientHealth: journal }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const SESSION = "tab-newer:session";
const ROW_HEIGHT = 50;
const VIEWPORT = 400;

class RecordingResizeObserver {
  static instances = new Set<RecordingResizeObserver>();
  constructor(private readonly callback: ResizeObserverCallback) { RecordingResizeObserver.instances.add(this); }
  observe(): void {}
  unobserve(): void {}
  disconnect(): void { RecordingResizeObserver.instances.delete(this); }
  deliver(): void { this.callback([], this as unknown as ResizeObserver); }
}

const items = (count: number) => conversationEntries(Array.from({ length: count },
  (_, index) => ({ id: `m-${index}`, type: "assistant" as const, content: `body-${index}` }))).map(entry => entry.item);

let all = items(20);
let hook: UseChatScrollResult;
let modes: string[];
let setAll: (next: ReturnType<typeof items>) => void;
let root: Root;
let host: HTMLElement;
const followTail = { current: true };

// Mirrors the TabSessionChat wiring: a detach freezes the window at its last row, a pin follows the tail.
function Harness() {
  const [rows, setRows] = useState(all);
  setAll = setRows;
  const [view, setView] = useState<{ anchor: string | null; size: number }>({ anchor: null, size: 100 });
  const projection = chatWindowProjection(rows, view.anchor, view.size);
  hook = useChatScroll({
    sessionKey: SESSION, active: true, loading: false, messageCount: projection.items.length,
    contentVersion: projection.items, prependVersion: projection.items[0] ? timelineItemKey(projection.items[0]) : null,
    followTailRef: followTail, journalTabId: "tab-newer", newerCount: projection.newerCount,
    onModeChange: (mode) => {
      modes.push(mode);
      setView(current => ({ ...current, anchor: mode === "pinned" ? null : timelineItemKey(projection.items[projection.items.length - 1]) }));
    },
    onNewerEdge: () => {
      const next = revealNewerMessages(rows, view.anchor, view.size);
      if (next) setView({ anchor: next.anchor, size: next.size });
      return next != null;
    },
  });
  return <div ref={hook.scrollRef} data-testid="scroll">
    {projection.items.map(item => <div key={timelineItemKey(item)} data-message-id={timelineItemKey(item)}
      data-source-message-id={timelineItemKey(item)} data-height={ROW_HEIGHT} />)}
  </div>;
}

function scroller(): HTMLElement { return host.querySelector<HTMLElement>('[data-testid="scroll"]')!; }
function rendered(): string[] { return Array.from(scroller().querySelectorAll<HTMLElement>("[data-message-id]")).map(row => row.dataset.messageId!); }
const lastMode = () => modes[modes.length - 1];
const contentHeight = (el: HTMLElement) => el.querySelectorAll("[data-height]").length * ROW_HEIGHT;

function stubGeometry(el: HTMLElement): void {
  let top = 0;
  const maxTop = () => Math.max(0, contentHeight(el) - VIEWPORT);
  Object.defineProperty(el, "clientHeight", { configurable: true, get: () => VIEWPORT });
  Object.defineProperty(el, "scrollHeight", { configurable: true, get: () => Math.max(VIEWPORT, contentHeight(el)) });
  Object.defineProperty(el, "scrollTop", { configurable: true, get: () => Math.min(top, maxTop()),
    set: (value: number) => { top = Math.max(0, Math.min(value, maxTop())); } });
}

let rectDescriptor: PropertyDescriptor | undefined;
beforeAll(() => {
  rectDescriptor = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "getBoundingClientRect");
  Object.defineProperty(HTMLElement.prototype, "getBoundingClientRect", { configurable: true, value(this: HTMLElement) {
    const parent = this.parentElement;
    const index = this.hasAttribute("data-height") && parent ? Array.from(parent.children).indexOf(this) : 0;
    const top = this.hasAttribute("data-height") ? index * ROW_HEIGHT - (parent?.scrollTop ?? 0) : 0;
    const height = this.hasAttribute("data-height") ? ROW_HEIGHT : VIEWPORT;
    return { top, bottom: top + height, height, left: 0, right: 800, width: 800, x: 0, y: top, toJSON: () => ({}) };
  } });
});
afterAll(() => { if (rectDescriptor) Object.defineProperty(HTMLElement.prototype, "getBoundingClientRect", rectDescriptor); });

beforeEach(async () => {
  vi.stubGlobal("ResizeObserver", RecordingResizeObserver);
  journal.mockReset();
  modes = [];
  followTail.current = true;
  all = items(20);
  clearPersistedChatScrollState(SESSION);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(<Harness />));
  stubGeometry(scroller());
  await act(async () => root.render(<Harness />));
  hook.followSend();
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});

describe("detached reader on the frozen bottom edge", () => {
  it("renders replies that arrive below it and lets a plain scroll down re-pin", async () => {
    await act(async () => { expect(hook.revealMessage("m-19")).toBe(true); });
    const el = scroller();
    expect(lastMode()).toBe("detached");
    expect(el.scrollHeight - el.scrollTop - el.clientHeight).toBe(0);
    const readerTop = el.scrollTop;

    await act(async () => setAll(items(23)));

    expect(rendered().slice(-3)).toEqual(["m-20", "m-21", "m-22"]);
    expect(el.scrollTop).toBe(readerTop);
    expect(lastMode()).toBe("detached");
    expect(journal.mock.calls.map(([event]) => [event.event, event.cause])).toEqual([
      ["chat-viewport-detached", "reveal"], ["chat-newer-revealed", "reveal"],
    ]);

    await act(async () => { for (const observer of RecordingResizeObserver.instances) observer.deliver(); });
    el.scrollTop = el.scrollHeight;
    await act(async () => hook.handleScroll({ currentTarget: el } as never));
    expect(lastMode()).toBe("pinned");
  });

  it("journals no user scroll detach and reveals nothing while the reader is above the edge", async () => {
    const el = scroller();
    await act(async () => { for (const observer of RecordingResizeObserver.instances) observer.deliver(); });
    el.scrollTop = 100;
    await act(async () => hook.handleScroll({ currentTarget: el } as never));
    expect(lastMode()).toBe("detached");

    await act(async () => setAll(items(23)));

    expect(rendered().slice(-1)[0]).toBe("m-19");
    expect(journal).not.toHaveBeenCalled();
  });
});
