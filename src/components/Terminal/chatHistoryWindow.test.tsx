// Run: npx tsx src/components/Terminal/chatHistoryWindow.test.tsx
// The transcript reducer driving the real scroll/window projection: bounded
// retention, top-only eviction, byte-cursor paging, and cache remount.
import {
  createTranscriptState,
  reduceTranscript,
  reduceToolEvents,
  TRANSCRIPT_RETENTION_LIMIT,
  transcriptBackfillCursor,
  transcriptCommittedEnd,
  transcriptRows,
  type NormalizedChatMessage,
  type TimelineItem,
  type TranscriptState,
} from "@codeterm/chat-core";
import { act, useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import {
  CHAT_WINDOW_SIZE,
  chatWindowProjection,
  chatWindowRange,
  getCachedChatHistoryWindow,
  setCachedChatHistoryWindow,
  slideChatWindowOlder,
  timelineItemKey,
} from "./chatWindow";
import { clearPersistedChatScrollState } from "./chatViewport";
import { useChatScroll } from "./useChatScroll";

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
}

class ResizeObserverStub {
  observe(): void {}
  disconnect(): void {}
}

const testWindow = new Window({ url: "http://localhost/chat-history-window" });
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true });
Object.defineProperty(globalThis, "window", { configurable: true, value: testWindow });
Object.defineProperty(globalThis, "document", { configurable: true, value: testWindow.document });
Object.defineProperty(globalThis, "navigator", { configurable: true, value: testWindow.navigator });
Object.defineProperty(globalThis, "HTMLElement", { configurable: true, value: testWindow.HTMLElement });
Object.defineProperty(globalThis, "Event", { configurable: true, value: testWindow.Event });
Object.defineProperty(globalThis, "ResizeObserver", { configurable: true, value: ResizeObserverStub });
Object.defineProperty(testWindow, "ResizeObserver", { configurable: true, value: ResizeObserverStub });
Object.defineProperty(testWindow.HTMLElement.prototype, "offsetTop", {
  configurable: true,
  get(this: HTMLElement) {
    if (!this.parentElement) return 0;
    return Array.from(this.parentElement.children).indexOf(this) * 20;
  },
});
Object.defineProperty(globalThis, "requestAnimationFrame", {
  configurable: true,
  value: (callback: FrameRequestCallback) => {
    callback(0);
    return 1;
  },
});
Object.defineProperty(globalThis, "cancelAnimationFrame", { configurable: true, value: () => {} });

/** Every source row occupies a fixed byte span, so a window's byte range and
 *  its row indexes are the same statement. */
const ROW_BYTES = 100;
const AUTHORITY = { sourceSessionId: "sid", sessionBindingNonce: "nonce" };

function message(index: number): NormalizedChatMessage {
  const base = {
    id: `m-${index}`,
    content: `message ${index}`,
    timestamp: String(10_000 + index),
    seq: index,
  };
  if (index % 20 === 0) {
    return { ...base, type: "tool_call", toolId: `tool-${index}`, toolName: "read", toolSummary: `Read ${index}` };
  }
  if (index % 20 === 1) {
    return { ...base, type: "tool_result", toolId: `tool-${index - 1}`, toolResult: `Result ${index}` };
  }
  if (index % 20 === 2) {
    return { ...base, type: "image", imageData: "AA==", imageMimeType: "image/png" };
  }
  if (index % 20 === 3) {
    return { ...base, type: "custom", inboundKind: "report", fromTab: "worker" };
  }
  return { ...base, type: index % 2 === 0 ? "assistant" : "user" };
}

const durableTranscript = Array.from({ length: 1_000 }, (_, index) => message(index));

function rowsIn(from: number, to: number): NormalizedChatMessage[] {
  return durableTranscript.slice(from, to);
}

function windowOf(from: number, to: number) {
  return { start: from * ROW_BYTES, end: to * ROW_BYTES };
}

const INITIAL_FROM = 1_000 - TRANSCRIPT_RETENTION_LIMIT;

function initialTranscript(): TranscriptState {
  return reduceTranscript(createTranscriptState(), {
    kind: "snapshot",
    authority: AUTHORITY,
    range: windowOf(INITIAL_FROM, 1_000),
    rows: rowsIn(INITIAL_FROM, 1_000),
  }).state;
}

interface HarnessView {
  transcript: TranscriptState;
  range: { start: number; end: number };
  timeline: TimelineItem[];
  scroll: HTMLDivElement;
  append: (rows: NormalizedChatMessage[], from: number) => void;
  loadOlder: () => void;
  handleScroll: () => void;
}

let latestView: HarnessView | null = null;

function HistoryHarness({ cacheKey }: { cacheKey: string }) {
  const [cached] = useState(() => getCachedChatHistoryWindow(cacheKey));
  const [transcript, setTranscript] = useState<TranscriptState>(
    () => cached?.transcript ?? initialTranscript(),
  );
  const [windowAnchorKey, setWindowAnchorKey] = useState<string | null>(cached?.anchorKey ?? null);
  const followTailRef = useRef(cached == null);
  const detachedReadingItemsRef = useRef<TimelineItem[]>([]);
  const pendingRevealRef = useRef(false);
  const rows = useMemo(() => transcriptRows(transcript), [transcript]);
  const timeline = useMemo(() => reduceToolEvents(rows), [rows]);
  const range = useMemo(() => chatWindowRange(timeline, windowAnchorKey), [timeline, windowAnchorKey]);
  const visible = useMemo(() => {
    const projection = chatWindowProjection(timeline, windowAnchorKey, detachedReadingItemsRef.current);
    detachedReadingItemsRef.current = projection.readingItems;
    return projection.items;
  }, [timeline, windowAnchorKey]);

  const handleModeChange = useCallback((mode: "pinned" | "detached") => {
    if (mode === "pinned") {
      setWindowAnchorKey(null);
      return;
    }
    setWindowAnchorKey((previous) => {
      const item = timeline[range.end - 1];
      return previous ?? (item ? timelineItemKey(item) : null);
    });
  }, [range.end, timeline]);

  const scrollAuthority = useChatScroll({
    sessionKey: cacheKey,
    active: true,
    loading: false,
    messageCount: visible.length,
    contentVersion: visible,
    prependVersion: visible[0] ? timelineItemKey(visible[0]) : null,
    followTailRef,
    onModeChange: handleModeChange,
  });

  useLayoutEffect(() => {
    if (!pendingRevealRef.current) return;
    pendingRevealRef.current = false;
    setWindowAnchorKey((previous) => slideChatWindowOlder(timeline, previous));
  }, [timeline]);

  const append = useCallback((incoming: NormalizedChatMessage[], from: number) => {
    setTranscript((previous) => reduceTranscript(previous, {
      kind: "delta",
      authority: AUTHORITY,
      range: windowOf(from, from + incoming.length),
      rows: incoming,
    }).state);
  }, []);

  const loadOlder = useCallback(() => {
    scrollAuthority.prepareForHistoryPrepend();
    if (range.start > 0) {
      setWindowAnchorKey((previous) => slideChatWindowOlder(timeline, previous));
      return;
    }
    setTranscript((previous) => {
      const cursor = transcriptBackfillCursor(previous);
      if (cursor == null || cursor <= 0) return previous;
      const pageEnd = cursor / ROW_BYTES;
      const pageStart = Math.max(0, pageEnd - 160);
      const next = reduceTranscript(previous, {
        kind: "backfill",
        authority: AUTHORITY,
        range: windowOf(pageStart, pageEnd),
        rows: rowsIn(pageStart, pageEnd),
      });
      if (next.outcome === "adopted") pendingRevealRef.current = true;
      return next.state;
    });
  }, [range.start, scrollAuthority, timeline]);

  useLayoutEffect(() => {
    setCachedChatHistoryWindow(
      cacheKey,
      windowAnchorKey ? { transcript, anchorKey: windowAnchorKey } : null,
    );
  }, [cacheKey, transcript, windowAnchorKey]);

  useLayoutEffect(() => {
    const scroll = scrollAuthority.scrollRef.current;
    if (!scroll) return;
    latestView = {
      transcript,
      range,
      timeline,
      scroll,
      append,
      loadOlder,
      handleScroll: () => scrollAuthority.handleScroll({ currentTarget: scroll } as never),
    };
  });

  return (
    <div ref={scrollAuthority.scrollRef} data-testid="scroll">
      <div>
        {visible.map((item) => {
          const rendered = item.kind === "message" ? item.message : item.event.message;
          return <div key={rendered.id} data-message-id={rendered.id}>{rendered.content}</div>;
        })}
      </div>
    </div>
  );
}

function geometry(scroll: HTMLDivElement): void {
  let scrollTop = 0;
  Object.defineProperty(scroll, "clientHeight", { configurable: true, get: () => 400 });
  Object.defineProperty(scroll, "scrollHeight", {
    configurable: true,
    get: () => scroll.querySelectorAll("[data-message-id]").length * 20,
  });
  Object.defineProperty(scroll, "scrollTop", {
    configurable: true,
    get: () => scrollTop,
    set: (value: number) => { scrollTop = value; },
  });
}

async function mount(cacheKey: string): Promise<{ root: Root; host: HTMLElement }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(<HistoryHarness cacheKey={cacheKey} />));
  assert(latestView, "history harness mounted");
  geometry(latestView.scroll);
  await act(async () => root.render(<HistoryHarness cacheKey={cacheKey} />));
  return { root, host };
}

async function main(): Promise<void> {
  const cacheKey = "history-window:session";
  setCachedChatHistoryWindow(cacheKey, null);
  clearPersistedChatScrollState(cacheKey);
  const mounted = await mount(cacheKey);
  const view = () => {
    assert(latestView, "history view exists");
    return latestView;
  };
  const rows = () => transcriptRows(view().transcript);

  assert(
    rows().length === TRANSCRIPT_RETENTION_LIMIT,
    "a bounded window retains only the hard message bound",
  );
  assert(
    document.querySelectorAll("[data-message-id]").length <= CHAT_WINDOW_SIZE,
    "rendered chat nodes stay within the hard row bound",
  );

  view().scroll.scrollTop = view().scroll.scrollHeight;
  const firstPinnedId = document.querySelector<HTMLElement>("[data-message-id]")?.dataset.messageId;
  await act(async () => view().append([message(1_000)], 1_000));
  assert(
    document.querySelector<HTMLElement>("[data-message-id]")?.dataset.messageId !== firstPinnedId,
    "pinned arrival evicts the oldest top row",
  );
  assert(
    view().scroll.scrollTop === view().scroll.scrollHeight,
    "pinned top eviction stays at the bottom in the same commit",
  );
  assert(
    rows()[0]?.id === `m-${INITIAL_FROM}` && rows()[rows().length - 1]?.id === "m-1000",
    "a single arrival slides the rendered window without discarding a parsed window",
  );

  view().scroll.scrollTop = 1_000;
  await act(async () => view().handleScroll());
  const detachedFirstId = document.querySelector<HTMLElement>("[data-message-id]")?.dataset.messageId;
  await act(async () => {
    view().append(Array.from({ length: 500 }, (_, index) => message(1_001 + index)), 1_001);
  });
  assert(
    document.querySelector<HTMLElement>("[data-message-id]")?.dataset.messageId === detachedFirstId,
    "detached capacity pressure preserves the stable reading projection",
  );
  assert(
    rows().length < 941 && view().transcript.evictedOlder > 0,
    "capacity pressure evicts whole parsed windows from the top",
  );
  assert(
    rows()[rows().length - 1]?.id === "m-1500",
    "detached arrivals always retain the newest live tail",
  );
  assert(
    transcriptCommittedEnd(view().transcript) === 1_501 * ROW_BYTES,
    "the committed byte end follows the live tail",
  );

  const retainedBefore = rows().length;
  await act(async () => view().append([message(900)], 900));
  assert(
    rows().length === retainedBefore && rows()[rows().length - 1]?.id === "m-1500",
    "a delta behind the committed end is ignored and never rewrites the tail",
  );

  await act(async () => view().loadOlder());
  await act(async () => view().loadOlder());
  view().scroll.scrollTop = 0;
  await act(async () => view().handleScroll());
  const refillAnchor = document.querySelector<HTMLElement>("[data-message-id]");
  assert(refillAnchor, "older refill has a visible anchor");
  const refillId = refillAnchor.dataset.messageId!;
  const cursorBeforePaging = transcriptBackfillCursor(view().transcript);
  for (let attempt = 0; attempt < 8; attempt += 1) {
    await act(async () => view().loadOlder());
    if ((transcriptBackfillCursor(view().transcript) ?? 0) < (cursorBeforePaging ?? 0)) break;
  }
  assert(
    rows()[rows().length - 1]?.id === "m-1500",
    "older paging never evicts the newest live tail",
  );
  assert(
    (transcriptBackfillCursor(view().transcript) ?? 0) < (cursorBeforePaging ?? 0),
    "older paging advances the byte cursor downward",
  );
  assert(
    document.querySelectorAll("[data-message-id]").length <= CHAT_WINDOW_SIZE * 2,
    "older refill keeps bounded reading and live-tail windows",
  );
  const refilledAnchor = document.querySelector<HTMLElement>(`[data-message-id="${refillId}"]`);
  assert(refilledAnchor, "older refill preserves the previously visible row");
  assert(
    view().scroll.scrollTop === refilledAnchor.offsetTop,
    "older refill compensates exactly for the prepended rows",
  );

  await act(async () => mounted.root.unmount());
  mounted.host.remove();
  latestView = null;
  const remounted = await mount(cacheKey);
  assert(
    document.querySelector(`[data-message-id="${refillId}"]`),
    "remount restores the bounded detached history window",
  );
  assert(
    document.querySelectorAll("[data-message-id]").length <= CHAT_WINDOW_SIZE * 2,
    "remount preserves bounded reading and live-tail windows",
  );
  await act(async () => remounted.root.unmount());
  remounted.host.remove();
}

main()
  .then(() => console.log("chat history window checks passed"))
  .catch((error) => {
    console.error(error);
    process.exit(1);
  });
