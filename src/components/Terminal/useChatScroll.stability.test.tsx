import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createTranscriptState,
  reduceTranscript,
  transcriptBackfillCursor,
  transcriptRows,
  type NormalizedChatMessage,
  type TimelineItem,
  type TranscriptState,
} from "@codeterm/chat-core";
import { clearPersistedChatScrollState } from "./chatViewport";
import {
  chatWindowAnchorIdentity,
  chatWindowRange,
  rekeyedChatWindowAnchor,
  timelineItemKey,
} from "./chatWindow";
import { useChatScroll, type UseChatScrollResult } from "./useChatScroll";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const SESSION = "tab-stability:session";
const AUTHORITY = { sourceSessionId: "sid", sessionBindingNonce: "nonce" };
const RETENTION_LIMIT = 24;
const ROW_BYTES = 100;
const ROW_HEIGHT = 50;
const VIEWPORT = 400;
const TRANSCRIPT_TYPES = ["assistant", "user", "tool_call", "tool_result", "image", "assistant"] as const;

type Batch = Parameters<typeof reduceTranscript>[1];

class RecordingResizeObserver {
  static instances = new Set<RecordingResizeObserver>();
  readonly targets = new Set<Element>();
  private readonly callback: ResizeObserverCallback;
  constructor(callback: ResizeObserverCallback) {
    this.callback = callback;
    RecordingResizeObserver.instances.add(this);
  }
  observe(target: Element): void { this.targets.add(target); }
  unobserve(target: Element): void { this.targets.delete(target); }
  disconnect(): void {
    this.targets.clear();
    RecordingResizeObserver.instances.delete(this);
  }
  deliver(): void { this.callback([], this as unknown as ResizeObserver); }
}

let transcript: TranscriptState;
let sourceLog: NormalizedChatMessage[];
let optimistic: NormalizedChatMessage[];
const heights = new Map<string, number>();
const followTail = { current: true };
let hook: UseChatScrollResult | null = null;
let root: Root | null = null;
let host: HTMLElement | null = null;

function sourceKey(row: NormalizedChatMessage): string {
  return row.sourceMessageId ?? row.id;
}

function Harness({ rows, active }: { rows: NormalizedChatMessage[]; active: boolean }) {
  hook = useChatScroll({
    sessionKey: SESSION,
    active,
    loading: false,
    messageCount: rows.length,
    contentVersion: rows,
    prependVersion: rows[0]?.id ?? null,
    followTailRef: followTail,
  });
  return (
    <div ref={hook.scrollRef} data-testid="scroll">
      <div>
        {rows.map((row) => (
          <div
            key={row.id}
            data-message-id={row.id}
            data-source-message-id={sourceKey(row)}
            data-height={heights.get(sourceKey(row)) ?? ROW_HEIGHT}
          >
            {row.content}
          </div>
        ))}
      </div>
    </div>
  );
}

function scroll(): HTMLElement {
  const el = host?.querySelector<HTMLElement>('[data-testid="scroll"]');
  if (!el) throw new Error("chat scroller is not mounted");
  return el;
}

function domRows(): HTMLElement[] {
  return Array.from(scroll().querySelectorAll<HTMLElement>("[data-message-id]"));
}

function rowBySource(source: string): HTMLElement {
  const row = scroll().querySelector<HTMLElement>(`[data-source-message-id="${source}"]`);
  if (!row) throw new Error(`no rendered row for source ${source}`);
  return row;
}

function stubScrollerGeometry(el: HTMLElement): void {
  let top = 0;
  const contentHeight = () => Array.from(el.querySelectorAll<HTMLElement>("[data-height]"))
    .reduce((sum, row) => sum + Number(row.getAttribute("data-height")), 0);
  const maxTop = () => Math.max(0, contentHeight() - VIEWPORT);
  Object.defineProperty(el, "clientHeight", { configurable: true, get: () => VIEWPORT });
  Object.defineProperty(el, "scrollHeight", { configurable: true, get: () => Math.max(VIEWPORT, contentHeight()) });
  Object.defineProperty(el, "scrollTop", {
    configurable: true,
    get: () => {
      top = Math.min(top, maxTop());
      return top;
    },
    set: (value: number) => { top = Math.max(0, Math.min(value, maxTop())); },
  });
}

function visibleRows(): NormalizedChatMessage[] {
  return [...transcriptRows(transcript), ...optimistic];
}

function sourceRow(type: string, extra: Partial<NormalizedChatMessage> = {}): NormalizedChatMessage {
  const index = sourceLog.length;
  const row = {
    id: `src-${index}`,
    type: type as NormalizedChatMessage["type"],
    content: `${type} ${index}`,
    timestamp: String(10_000 + index),
    seq: index,
    ...extra,
  } as NormalizedChatMessage;
  sourceLog.push(row);
  return row;
}

function commit(batch: Batch): void {
  const result = reduceTranscript(transcript, batch, RETENTION_LIMIT);
  expect(result.outcome).toBe("adopted");
  transcript = result.state;
}

function appendDelta(row: NormalizedChatMessage): void {
  const start = (row.seq ?? 0) * ROW_BYTES;
  commit({ kind: "delta", authority: AUTHORITY, range: { start, end: start + ROW_BYTES }, rows: [row] });
}

function fill(count: number): void {
  for (let index = 0; index < count; index += 1) {
    appendDelta(sourceRow(TRANSCRIPT_TYPES[index % TRANSCRIPT_TYPES.length]));
  }
}

function rekeyRetainedTail(): void {
  const start = transcriptBackfillCursor(transcript);
  if (start == null) throw new Error("no retained range to re-key");
  commit({
    kind: "snapshot",
    authority: AUTHORITY,
    range: { start, end: sourceLog.length * ROW_BYTES },
    rows: sourceLog.slice(start / ROW_BYTES),
  });
}

async function mount(): Promise<void> {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(<Harness rows={visibleRows()} active={false} />));
  stubScrollerGeometry(scroll());
  await act(async () => root!.render(<Harness rows={visibleRows()} active />));
}

async function unmount(): Promise<void> {
  if (root) await act(async () => root!.unmount());
  host?.remove();
  root = null;
  host = null;
}

async function update(): Promise<void> {
  await act(async () => root!.render(<Harness rows={visibleRows()} active />));
}

async function userScrollTo(scrollTop: number): Promise<void> {
  scroll().scrollTop = scrollTop;
  await deliverScrollEvent();
}

async function deliverScrollEvent(): Promise<void> {
  await act(async () => hook!.handleScroll({ currentTarget: scroll() } as never));
}

async function asyncHeightChange(source: string, height: number): Promise<void> {
  heights.set(source, height);
  rowBySource(source).setAttribute("data-height", String(height));
  await act(async () => {
    for (const observer of RecordingResizeObserver.instances) observer.deliver();
  });
}

function expectPinnedToBottom(): void {
  const el = scroll();
  expect(el.scrollTop).toBe(el.scrollHeight - el.clientHeight);
  expect(followTail.current).toBe(true);
  expect(el.style.overflowAnchor).toBe("none");
}

function readerPosition(source: string): number {
  return rowBySource(source).offsetTop - scroll().scrollTop;
}

let offsetTopDescriptor: PropertyDescriptor | undefined;

beforeAll(() => {
  offsetTopDescriptor = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "offsetTop");
  Object.defineProperty(HTMLElement.prototype, "offsetTop", {
    configurable: true,
    get(this: HTMLElement) {
      if (!this.hasAttribute("data-height") || !this.parentElement) return 0;
      let top = 0;
      for (const sibling of Array.from(this.parentElement.children)) {
        if (sibling === this) break;
        top += Number(sibling.getAttribute("data-height") ?? 0);
      }
      return top;
    },
  });
  vi.stubGlobal("ResizeObserver", RecordingResizeObserver);
});

afterAll(() => {
  if (offsetTopDescriptor) Object.defineProperty(HTMLElement.prototype, "offsetTop", offsetTopDescriptor);
  else delete (HTMLElement.prototype as { offsetTop?: number }).offsetTop;
  vi.unstubAllGlobals();
});

beforeEach(() => {
  transcript = createTranscriptState();
  sourceLog = [];
  optimistic = [];
  heights.clear();
  followTail.current = true;
  clearPersistedChatScrollState(SESSION);
  fill(40);
});

afterEach(async () => {
  await unmount();
  clearPersistedChatScrollState(SESSION);
});

describe("attached tail at the history cap", () => {
  it("stays at the bottom through optimistic send, committed turn, streamed rows, eviction and async growth", async () => {
    await mount();
    expect(transcript.evictedOlder).toBeGreaterThan(0);
    expectPinnedToBottom();
    expect([...RecordingResizeObserver.instances].some((observer) => observer.targets.has(scroll().firstElementChild!)))
      .toBe(true);

    optimistic = [{ id: "opt-1", type: "user", content: "ship it", correlationId: "opt-1" }];
    await act(async () => hook!.followSend());
    await update();
    expectPinnedToBottom();

    const evictedBeforeCommit = transcript.evictedOlder;
    optimistic = [];
    appendDelta(sourceRow("user", { correlationId: "opt-1" }));
    await update();
    expect(transcript.evictedOlder).toBeGreaterThan(evictedBeforeCommit);
    expectPinnedToBottom();

    for (const streamed of [
      sourceRow("assistant"),
      sourceRow("tool_call", { toolName: "Bash" }),
      sourceRow("tool_result"),
      sourceRow("tool_call", { toolName: "Edit" }),
    ]) {
      appendDelta(streamed);
      await update();
      await deliverScrollEvent();
      expectPinnedToBottom();
    }

    rekeyRetainedTail();
    await update();
    expectPinnedToBottom();

    const tail = domRows();
    await asyncHeightChange(tail[tail.length - 1].dataset.sourceMessageId!, 320);
    expectPinnedToBottom();
    await asyncHeightChange(tail[2].dataset.sourceMessageId!, 260);
    expectPinnedToBottom();
  });
});

describe("detached reader at the history cap", () => {
  it("stays on the same content through optimistic send, eviction above, streamed rows and render-id re-key", async () => {
    await mount();
    await userScrollTo(500);
    expect(followTail.current).toBe(false);
    const anchor = domRows()[10].dataset.sourceMessageId!;
    const position = readerPosition(anchor);

    optimistic = [{ id: "opt-2", type: "user", content: "while reading", correlationId: "opt-2" }];
    await update();
    expect(readerPosition(anchor)).toBe(position);

    const evictedBefore = transcript.evictedOlder;
    const firstRenderedBefore = domRows()[0].dataset.messageId;
    optimistic = [];
    appendDelta(sourceRow("user", { correlationId: "opt-2" }));
    await update();
    expect(transcript.evictedOlder).toBeGreaterThan(evictedBefore);
    expect(domRows()[0].dataset.messageId).not.toBe(firstRenderedBefore);
    expect(readerPosition(anchor)).toBe(position);

    for (const streamed of [
      sourceRow("assistant"),
      sourceRow("tool_call", { toolName: "Bash" }),
      sourceRow("tool_result"),
      sourceRow("tool_call", { toolName: "Edit" }),
    ]) {
      appendDelta(streamed);
      await update();
      expect(readerPosition(anchor)).toBe(position);
    }

    const renderIdBefore = rowBySource(anchor).dataset.messageId;
    rekeyRetainedTail();
    await update();
    expect(rowBySource(anchor).dataset.messageId).not.toBe(renderIdBefore);
    expect(readerPosition(anchor)).toBe(position);

    await deliverScrollEvent();
    expect(followTail.current).toBe(false);
    expect(readerPosition(anchor)).toBe(position);
    expect(scroll().style.overflowAnchor).toBe("none");
  });

  it("does not move for async image, HTML preview and Mermaid height changes above or below the reader", async () => {
    await mount();
    await userScrollTo(500);
    const rows = domRows();
    const anchor = rows[10].dataset.sourceMessageId!;
    const position = readerPosition(anchor);

    await asyncHeightChange(rows[2].dataset.sourceMessageId!, 260);
    expect(readerPosition(anchor)).toBe(position);
    await asyncHeightChange(rows[3].dataset.sourceMessageId!, 180);
    expect(readerPosition(anchor)).toBe(position);
    await asyncHeightChange(rows[9].dataset.sourceMessageId!, 340);
    expect(readerPosition(anchor)).toBe(position);
    await asyncHeightChange(rows[14].dataset.sourceMessageId!, 400);
    expect(readerPosition(anchor)).toBe(position);

    await deliverScrollEvent();
    expect(followTail.current).toBe(false);
    expect(readerPosition(anchor)).toBe(position);
  });

  it("keeps a detached reader detached when compensation lands on the bottom edge", async () => {
    await mount();
    await userScrollTo(790);
    expect(followTail.current).toBe(false);
    const rows = domRows();
    const anchor = rows[16].dataset.sourceMessageId!;
    const position = readerPosition(anchor);

    heights.set(rows[0].dataset.sourceMessageId!, ROW_HEIGHT + 10);
    rows[0].setAttribute("data-height", String(ROW_HEIGHT + 10));
    await asyncHeightChange(rows[rows.length - 1].dataset.sourceMessageId!, ROW_HEIGHT - 10);
    expect(readerPosition(anchor)).toBe(position);
    expect(scroll().scrollTop).toBe(scroll().scrollHeight - scroll().clientHeight);

    await deliverScrollEvent();
    expect(followTail.current).toBe(false);
  });
});

describe("intentional viewport movement", () => {
  it("moves for exact reveal and scroll-to-latest, then holds or follows accordingly", async () => {
    await mount();
    await userScrollTo(500);
    const target = domRows()[2].dataset.sourceMessageId!;

    let revealed = false;
    await act(async () => { revealed = hook!.revealMessage(target); });
    expect(revealed).toBe(true);
    expect(scroll().scrollTop).toBe(rowBySource(target).offsetTop);
    expect(followTail.current).toBe(false);

    appendDelta(sourceRow("assistant"));
    await update();
    expect(readerPosition(target)).toBe(0);

    await act(async () => hook!.scrollToBottom());
    expectPinnedToBottom();
    appendDelta(sourceRow("assistant"));
    await update();
    expectPinnedToBottom();
  });

  it("keeps older-history prepend on the same content", async () => {
    await mount();
    await userScrollTo(500);
    const anchor = domRows()[10].dataset.sourceMessageId!;
    const position = readerPosition(anchor);

    await act(async () => hook!.prepareForHistoryPrepend());
    const end = transcriptBackfillCursor(transcript)!;
    const start = end - 8 * ROW_BYTES;
    commit({
      kind: "backfill",
      authority: AUTHORITY,
      range: { start, end },
      rows: sourceLog.slice(start / ROW_BYTES, end / ROW_BYTES),
    });
    await update();
    expect(domRows()).toHaveLength(RETENTION_LIMIT + 8);
    expect(readerPosition(anchor)).toBe(position);
    expect(scroll().scrollTop).toBe(500 + 8 * ROW_HEIGHT);
  });

  it("restores a persisted detached position on remount", async () => {
    await mount();
    await userScrollTo(500);
    const anchor = domRows()[10].dataset.sourceMessageId!;
    appendDelta(sourceRow("assistant"));
    await update();
    const position = readerPosition(anchor);
    const persistedTop = scroll().scrollTop;

    await unmount();
    await mount();
    expect(scroll().scrollTop).toBe(persistedTop);
    expect(followTail.current).toBe(false);
    expect(readerPosition(anchor)).toBe(position);
  });
});

describe("bounded window anchor identity", () => {
  function timeline(): TimelineItem[] {
    return visibleRows().map((message) => ({ kind: "message" as const, message }));
  }

  it("follows the same source row through a render-id re-key", () => {
    const items = timeline();
    const anchorKey = timelineItemKey(items[6]);
    const identity = chatWindowAnchorIdentity(items, anchorKey)!;

    rekeyRetainedTail();
    const rekeyedItems = timeline();
    expect(rekeyedItems.some((item) => timelineItemKey(item) === anchorKey)).toBe(false);
    const rekeyed = rekeyedChatWindowAnchor(rekeyedItems, identity);
    expect(rekeyed).not.toBeNull();
    expect(rekeyed).not.toBe(anchorKey);
    const end = chatWindowRange(rekeyedItems, rekeyed).end;
    expect(sourceKey((rekeyedItems[end - 1] as { message: NormalizedChatMessage }).message)).toBe(identity.sourceId);
  });

  it("follows an optimistic anchor onto its committed turn and releases an evicted anchor", () => {
    optimistic = [{ id: "opt-3", type: "user", content: "anchor", correlationId: "opt-3" }];
    const withOptimistic = timeline();
    const optimisticIdentity = chatWindowAnchorIdentity(withOptimistic, "opt-3")!;
    const oldestIdentity = chatWindowAnchorIdentity(withOptimistic, timelineItemKey(withOptimistic[0]))!;

    optimistic = [];
    const committed = sourceRow("user", { correlationId: "opt-3" });
    appendDelta(committed);
    const items = timeline();
    const rekeyed = rekeyedChatWindowAnchor(items, optimisticIdentity);
    expect(rekeyed).toBe(timelineItemKey(items[items.length - 1]));
    expect(rekeyedChatWindowAnchor(items, oldestIdentity)).toBeNull();
  });
});
