import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { clearPersistedChatScrollState } from "./chatViewport";
import { useChatScroll, type UseChatScrollResult } from "./useChatScroll";

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
}

function assertScrollTop(element: HTMLElement, expected: number, label: string): void {
  assert(element.scrollTop === expected, label);
}

const hookWindow = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true });
Object.defineProperty(globalThis, "window", { configurable: true, value: hookWindow });
Object.defineProperty(globalThis, "document", { configurable: true, value: hookWindow.document });
Object.defineProperty(globalThis, "navigator", { configurable: true, value: hookWindow.navigator });
Object.defineProperty(globalThis, "HTMLElement", { configurable: true, value: hookWindow.HTMLElement });

interface Geometry {
  scrollHeight: number;
  clientHeight: number;
}

function setGeometry(el: HTMLElement, geometry: Geometry): void {
  let scrollTop = el.scrollTop;
  Object.defineProperty(el, "scrollTop", {
    configurable: true,
    get: () => scrollTop,
    set: (value: number) => { scrollTop = Math.min(value, geometry.scrollHeight - geometry.clientHeight); },
  });
  Object.defineProperty(el, "scrollHeight", { configurable: true, get: () => geometry.scrollHeight });
  Object.defineProperty(el, "clientHeight", { configurable: true, get: () => geometry.clientHeight });
}

let hookResult: UseChatScrollResult | null = null;
const followTail = { current: true };
let resizeCallback: ResizeObserverCallback | null = null;

class MockWebKitResizeObserver {
  constructor(callback: ResizeObserverCallback) {
    resizeCallback = callback;
  }

  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}

function Fixture({ revision, messageCount, firstId }: { revision: number; messageCount: number; firstId: string }) {
  hookResult = useChatScroll({
    sessionKey: "tab:session",
    active: true,
    loading: false,
    messageCount,
    contentVersion: revision,
    prependVersion: firstId,
    followTailRef: followTail,
  });
  return createElement(
    "div",
    { ref: hookResult.scrollRef, "data-testid": "scroll" },
    ...Array.from({ length: messageCount }, (_, index) =>
      createElement("output", { key: index, "data-testid": "transcript-row" }, String(index)),
    ),
  );
}

async function render(root: Root, revision: number, messageCount: number, firstId = "m-1"): Promise<void> {
  await act(async () => root.render(createElement(Fixture, { revision, messageCount, firstId })));
}

async function main(): Promise<void> {
  clearPersistedChatScrollState("tab:session");
  const previousResizeObserver = globalThis.ResizeObserver;
  Object.defineProperty(globalThis, "ResizeObserver", {
    configurable: true,
    value: MockWebKitResizeObserver,
  });
  const previousWarn = console.warn;
  const observerFaults: unknown[][] = [];
  console.warn = (...args: unknown[]) => observerFaults.push(args);
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  try {
    await render(root, 0, 0);
    const scroll = host.querySelector('[data-testid="scroll"]') as HTMLElement;
    setGeometry(scroll, { scrollHeight: 1000, clientHeight: 400 });
    await render(root, 1, 1);
    assertScrollTop(scroll, 600, "initial pinned transcript opens at latest");
    assert(scroll.style.overflowAnchor === "none", "native anchoring cannot become a second writer");

    await act(async () => resizeCallback?.([{
      target: scroll,
      contentRect: { width: 0, height: 0 },
    }] as ResizeObserverEntry[], {} as ResizeObserver));
    assertScrollTop(scroll, 600, "zero-sized WebKitGTK observation moved the pinned viewport");
    Object.defineProperty(scroll, "scrollHeight", {
      configurable: true,
      get: () => { throw new Error("detached WebKitGTK layout"); },
    });
    await act(async () => resizeCallback?.([{
      target: scroll,
      contentRect: { width: 1, height: 1 },
    }] as ResizeObserverEntry[], {} as ResizeObserver));
    assert(observerFaults.length === 1, "ResizeObserver failure was swallowed without an observable recovery fault");
    setGeometry(scroll, { scrollHeight: 1200, clientHeight: 400 });
    await render(root, 2, 2);
    assert(
      host.querySelectorAll('[data-testid="transcript-row"]').length === 2,
      "a WebKitGTK ResizeObserver fault halted later transcript rows",
    );

    scroll.scrollTop = 799.9;
    await act(async () => hookResult!.handleScroll({ currentTarget: scroll } as never));
    assert(followTail.current, "a fractional layout clamp at the bottom keeps the live tail pinned");

    scroll.scrollTop = 799;
    await act(async () => hookResult!.handleScroll({ currentTarget: scroll } as never));
    assert(!followTail.current, "a real upward movement detaches immediately");

    await act(async () => hookResult!.prepareForHistoryPrepend());
    setGeometry(scroll, { scrollHeight: 1250, clientHeight: 400 });
    await render(root, 3, 3);
    assertScrollTop(scroll, 799, "an intervening live update does not consume or move the prepend snapshot");
    setGeometry(scroll, { scrollHeight: 1550, clientHeight: 400 });
    await render(root, 4, 4, "m-0");
    assertScrollTop(scroll, 1099, "older-history prepend preserves the visible position");

    await act(async () => hookResult!.scrollToBottom());
    assertScrollTop(scroll, 1150, "explicit down returns to latest");
    setGeometry(scroll, { scrollHeight: 1750, clientHeight: 400 });
    await render(root, 5, 5, "m-0");
    assertScrollTop(scroll, 1350, "pinned viewport follows new canonical content");
    console.log("chat scroll authority checks passed");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    console.warn = previousWarn;
    Object.defineProperty(globalThis, "ResizeObserver", {
      configurable: true,
      value: previousResizeObserver,
    });
  }
}

void main().catch((error) => {
  console.error(error);
  process.exit(1);
});
