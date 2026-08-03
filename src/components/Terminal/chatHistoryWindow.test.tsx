import {
  mergeById,
  reduceToolEvents,
  type NormalizedChatMessage,
  type TimelineItem,
} from "@codeterm/chat-core";
import { act, useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import {
  CHAT_RETAINED_MESSAGE_LIMIT,
  CHAT_WINDOW_SIZE,
  chatWindowRange,
  getCachedChatHistoryWindow,
  mergeRetainedChatLive,
  refillRetainedChatHistory,
  retainChatSnapshotTail,
  seekRetainedChatPredecessor,
  setCachedChatHistoryWindow,
  slideChatWindowOlder,
  timelineItemKey,
  type RetainedChatHistory,
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

interface HarnessView {
  history: RetainedChatHistory;
  range: { start: number; end: number };
  timeline: TimelineItem[];
  scroll: HTMLDivElement;
  append: (messages: NormalizedChatMessage[]) => void;
  loadOlder: () => void;
  handleScroll: () => void;
}

let latestView: HarnessView | null = null;

function HistoryHarness({ cacheKey }: { cacheKey: string }) {
  const [cached] = useState(() => getCachedChatHistoryWindow(cacheKey));
  const [history, setHistory] = useState(() => cached?.history ?? retainChatSnapshotTail(durableTranscript));
  const [windowAnchorKey, setWindowAnchorKey] = useState<string | null>(cached?.anchorKey ?? null);
  const followTailRef = useRef(cached == null);
  const pendingRevealRef = useRef(false);
  const timeline = useMemo(() => reduceToolEvents(history.messages), [history.messages]);
  const range = useMemo(() => chatWindowRange(timeline, windowAnchorKey), [timeline, windowAnchorKey]);
  const visible = timeline.slice(range.start, range.end);

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

  const append = useCallback((incoming: NormalizedChatMessage[]) => {
    setHistory((previous) => {
      const merged = mergeById(previous.messages, incoming);
      return mergeRetainedChatLive(previous, merged);
    });
  }, []);

  const loadOlder = useCallback(() => {
    scrollAuthority.prepareForHistoryPrepend();
    if (range.start > 0) {
      setWindowAnchorKey((previous) => slideChatWindowOlder(timeline, previous));
      return;
    }
    pendingRevealRef.current = true;
    setHistory((previous) => refillRetainedChatHistory(previous, durableTranscript));
  }, [range.start, scrollAuthority, timeline]);

  useLayoutEffect(() => {
    setCachedChatHistoryWindow(
      cacheKey,
      windowAnchorKey ? { history, anchorKey: windowAnchorKey } : null,
    );
  }, [cacheKey, history, windowAnchorKey]);

  useLayoutEffect(() => {
    const scroll = scrollAuthority.scrollRef.current;
    if (!scroll) return;
    latestView = {
      history,
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

  assert(
    view().history.messages.length === CHAT_RETAINED_MESSAGE_LIMIT,
    "1,000 mixed rows retain only the hard message window",
  );
  assert(
    document.querySelectorAll("[data-message-id]").length <= CHAT_WINDOW_SIZE,
    "rendered chat nodes stay within the hard row bound",
  );

  const originalDetached = retainChatSnapshotTail(
    durableTranscript,
    CHAT_RETAINED_MESSAGE_LIMIT,
    {
      startOffset: 100,
      endOffset: 1_000,
      seekBeforeOffset: 1_000,
      pageAnchorId: null,
      boundaryVerified: false,
    },
  );
  const detachedAnchor = originalDetached.messages.find((item) => item.id === "m-700");
  assert(detachedAnchor, "inverse fixture has a retained detached anchor");
  const refreshedTail = Array.from({ length: 1_000 }, (_, index) => message(2_000 + index));
  const missingRefresh = refillRetainedChatHistory(originalDetached, refreshedTail);
  assert(missingRefresh === originalDetached, "missing refreshed anchor leaves retained history unchanged");
  assert(missingRefresh.omittedOlder === 560, "missing refreshed anchor preserves omitted older count");
  assert(missingRefresh.predecessorRange?.startOffset === 100, "missing refreshed anchor preserves durable cursor");
  assert(missingRefresh.messages[0]?.id === "m-560", "newer refreshed rows never prepend ahead of detached history");
  assert(
    missingRefresh.messages.find((item) => item.id === "m-700") === detachedAnchor,
    "missing refreshed anchor preserves row identity",
  );

  const missedBoundaryPage = durableTranscript.slice(800, 1_000);
  const missedBoundary = seekRetainedChatPredecessor(missingRefresh, missedBoundaryPage, 800);
  assert(!missedBoundary.prepended, "a page newer than the retained boundary prepends nothing");
  assert(missedBoundary.history.messages[0]?.id === "m-560", "failed seek preserves chronology");
  assert(missedBoundary.history.predecessorRange?.startOffset === 100, "failed seek preserves durable cursor");
  assert(missedBoundary.history.predecessorRange?.seekBeforeOffset === 800, "failed seek advances only its range cursor");

  const emptyBoundary = seekRetainedChatPredecessor(missedBoundary.history, [], 700);
  assert(!emptyBoundary.prepended, "D14 empty boundary prepends nothing");
  assert(emptyBoundary.history.omittedOlder === 560, "D14 empty boundary preserves omitted history");
  assert(emptyBoundary.history.predecessorRange?.startOffset === 100, "D14 empty boundary preserves durable cursor");

  const boundaryPage = durableTranscript.slice(500, 700);
  const foundBoundary = seekRetainedChatPredecessor(emptyBoundary.history, boundaryPage, 500);
  assert(foundBoundary.prepended, "later Show older finds the exact retained boundary");
  assert(foundBoundary.history.messages[0]?.id === "m-500", "boundary refill prepends only chronological predecessors");
  assert(foundBoundary.history.predecessorRange?.startOffset === 100, "boundary refill keeps the durable cursor");
  assert(
    foundBoundary.history.messages.find((item) => item.id === "m-700") === detachedAnchor,
    "boundary refill preserves the detached anchor identity",
  );

  const priorBoundaryPage = durableTranscript.slice(400, 500);
  const continuedOlder = seekRetainedChatPredecessor(foundBoundary.history, priorBoundaryPage, 400);
  assert(continuedOlder.prepended, "subsequent Show older continues from the verified page boundary");
  assert(continuedOlder.history.messages[0]?.id === "m-400", "continued refill remains chronological");
  assert(continuedOlder.history.predecessorRange?.seekBeforeOffset === 400, "continued refill advances exact range");

  view().scroll.scrollTop = view().scroll.scrollHeight;
  const firstPinnedId = document.querySelector<HTMLElement>("[data-message-id]")?.dataset.messageId;
  await act(async () => view().append([message(1_000)]));
  assert(
    document.querySelector<HTMLElement>("[data-message-id]")?.dataset.messageId !== firstPinnedId,
    "pinned arrival evicts the oldest top row",
  );
  assert(
    view().scroll.scrollTop === view().scroll.scrollHeight,
    "pinned top eviction stays at the bottom in the same commit",
  );

  view().scroll.scrollTop = 1_000;
  await act(async () => view().handleScroll());
  const anchored = Array.from(document.querySelectorAll<HTMLElement>("[data-message-id]"))
    .find((node) => node.offsetTop <= view().scroll.scrollTop && node.offsetTop + 20 > view().scroll.scrollTop);
  assert(anchored, "detached viewport has an exact visible anchor");
  const detachedFirstId = document.querySelector<HTMLElement>("[data-message-id]")?.dataset.messageId;
  await act(async () => {
    view().append(Array.from({ length: 500 }, (_, index) => message(1_001 + index)));
  });
  assert(
    document.querySelector<HTMLElement>("[data-message-id]")?.dataset.messageId !== detachedFirstId,
    "detached capacity pressure evicts only the oldest top rows",
  );
  assert(
    view().history.messages.length <= CHAT_RETAINED_MESSAGE_LIMIT,
    "detached arrivals remain bounded",
  );
  assert(
    view().history.messages[view().history.messages.length - 1]?.id === "m-1500",
    "detached arrivals always retain the newest live tail",
  );

  const streamingNode = document.querySelector<HTMLElement>("[data-message-id]");
  assert(streamingNode, "bounded detached window keeps visible tail rows");
  const streamingId = streamingNode.dataset.messageId!;
  const streamed = {
    ...view().history.messages.find((item) => item.id === streamingId)!,
    content: "streamed replacement",
  };
  await act(async () => view().append([streamed]));
  assert(
    document.querySelector(`[data-message-id="${streamingId}"]`) === streamingNode,
    "streaming replacement keeps row identity",
  );
  assert(streamingNode.textContent === "streamed replacement", "streaming replacement updates the retained row");

  await act(async () => view().loadOlder());
  await act(async () => view().loadOlder());
  view().scroll.scrollTop = 0;
  await act(async () => view().handleScroll());
  const refillAnchor = document.querySelector<HTMLElement>("[data-message-id]");
  assert(refillAnchor, "older refill has a visible anchor");
  const refillId = refillAnchor.dataset.messageId!;
  await act(async () => view().loadOlder());
  assert(
    view().history.messages[view().history.messages.length - 1]?.id === "m-1500",
    "older refill never evicts the newest live tail",
  );
  assert(
    document.querySelectorAll("[data-message-id]").length <= CHAT_WINDOW_SIZE,
    "older refill respects the rendered hard bound",
  );
  const refilledAnchor = document.querySelector<HTMLElement>(`[data-message-id="${refillId}"]`);
  assert(refilledAnchor, "older refill preserves the previously visible row");
  assert(
    view().scroll.scrollTop === refilledAnchor.offsetTop,
    "older refill compensates exactly for the prepended rows",
  );

  const remountAnchor = document.querySelector<HTMLElement>(`[data-message-id="${refillId}"]`);
  assert(remountAnchor, "remount anchor is retained before unmount");
  await act(async () => mounted.root.unmount());
  mounted.host.remove();
  latestView = null;
  const remounted = await mount(cacheKey);
  assert(
    document.querySelector(`[data-message-id="${refillId}"]`),
    "remount restores the bounded detached history window",
  );
  assert(
    document.querySelectorAll("[data-message-id]").length <= CHAT_WINDOW_SIZE,
    "remount preserves the rendered hard bound",
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
