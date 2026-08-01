import { act, createElement, useRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { useChatScroll, type UseChatScrollResult } from "./useChatScroll";
import { clearPersistedChatScrollState } from "./chatViewport";

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
}

const hookWindow = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true });
Object.defineProperty(globalThis, "window", { configurable: true, value: hookWindow });
Object.defineProperty(globalThis, "document", { configurable: true, value: hookWindow.document });
Object.defineProperty(globalThis, "navigator", { configurable: true, value: hookWindow.navigator });
Object.defineProperty(globalThis, "HTMLElement", { configurable: true, value: hookWindow.HTMLElement });
Object.defineProperty(globalThis, "Event", { configurable: true, value: hookWindow.Event });

class FakeResizeObserver {
  static instances: FakeResizeObserver[] = [];
  targets: Element[] = [];
  private callback: ResizeObserverCallback;

  constructor(callback: ResizeObserverCallback) {
    this.callback = callback;
    FakeResizeObserver.instances.push(this);
  }

  observe(target: Element): void {
    this.targets.push(target);
  }

  unobserve(target: Element): void {
    this.targets = this.targets.filter((t) => t !== target);
  }

  disconnect(): void {
    this.targets = [];
  }

  trigger(): void {
    this.callback([], this as unknown as ResizeObserver);
  }
}
Object.defineProperty(globalThis, "ResizeObserver", { configurable: true, value: FakeResizeObserver });
Object.defineProperty(hookWindow, "ResizeObserver", { configurable: true, value: FakeResizeObserver });

const rafQueue = new Map<number, FrameRequestCallback>();
let rafId = 0;
const installRaf = (target: object) => {
  Object.defineProperty(target, "requestAnimationFrame", {
    configurable: true,
    value: (cb: FrameRequestCallback) => {
      rafId += 1;
      rafQueue.set(rafId, cb);
      return rafId;
    },
  });
  Object.defineProperty(target, "cancelAnimationFrame", {
    configurable: true,
    value: (id: number) => rafQueue.delete(id),
  });
};
installRaf(globalThis);
installRaf(hookWindow);

function flushRaf(): void {
  const pending = [...rafQueue.values()];
  rafQueue.clear();
  for (const cb of pending) cb(0);
}

interface Geometry {
  scrollHeight: number;
  clientHeight: number;
}

function setGeometry(el: HTMLElement, geometry: Geometry): void {
  let scrollTop = (el as unknown as { __scrollTop?: number }).__scrollTop ?? 0;
  Object.defineProperty(el, "__scrollTop", { configurable: true, get: () => scrollTop, set: (v: number) => { scrollTop = v; } });
  Object.defineProperty(el, "scrollTop", { configurable: true, get: () => scrollTop, set: (v: number) => { scrollTop = v; } });
  Object.defineProperty(el, "scrollHeight", { configurable: true, get: () => geometry.scrollHeight });
  Object.defineProperty(el, "clientHeight", { configurable: true, get: () => geometry.clientHeight });
}

function countScrollTopWrites(el: HTMLElement): { reset: () => void; value: () => number } {
  let scrollTop = el.scrollTop;
  let writes = 0;
  Object.defineProperty(el, "scrollTop", {
    configurable: true,
    get: () => scrollTop,
    set: (value: number) => {
      writes += 1;
      scrollTop = value;
    },
  });
  return { reset: () => { writes = 0; }, value: () => writes };
}

let hookResult: UseChatScrollResult | null = null;

function Fixture({ loading, messageCount }: { loading: boolean; messageCount: number }) {
  const scroll = useChatScroll({ sessionKey: "tab:sess", active: true, loading, messageCount });
  hookResult = scroll;
  const messageIds = useRef(["m-1", "m-2", "m-3"]);
  if (loading) return createElement("div", null, "loading");
  return createElement(
    "div",
    { ref: scroll.scrollRef, "data-testid": "scroll" },
    createElement(
      "div",
      { ref: scroll.contentRef, "data-testid": "content" },
      ...messageIds.current.slice(0, messageCount).map((id) =>
        createElement("div", { key: id, "data-message-id": id }, id),
      ),
    ),
  );
}

async function renderFixture(root: Root, loading: boolean, messageCount: number): Promise<void> {
  await act(async () => {
    root.render(createElement(Fixture, { loading, messageCount }));
  });
}

function observerFor(el: Element): FakeResizeObserver | undefined {
  return FakeResizeObserver.instances.find((o) => o.targets.includes(el));
}

async function main(): Promise<void> {
// --- 1. observer attaches even when the transcript mounts behind a loading gate ---
// (TabSessionChat's first render always has loading=true and early-returns, so
// the scroll/content nodes do not exist when the hook's effects first run.)
{
  clearPersistedChatScrollState("tab:sess");
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await renderFixture(root, true, 0);
  await renderFixture(root, false, 3);

  const content = host.querySelector('[data-testid="content"]') as HTMLElement;
  const scroll = host.querySelector('[data-testid="scroll"]') as HTMLElement;
  assert(content && scroll, "fixture mounted transcript nodes");
  assert(
    observerFor(content) != null,
    "content observer attaches after the loading gate opens (bottom-follow must not silently die)",
  );
  assert(
    observerFor(scroll) != null,
    "scroll container observer attaches so viewport resizes re-anchor the pinned transcript",
  );

  // --- 2. pinned sync writes scrollTop synchronously inside the observer callback ---
  // ResizeObserver delivers after layout and before paint; deferring the write to
  // rAF paints one stale frame per streamed chunk (the visible one-frame jump).
  setGeometry(scroll, { scrollHeight: 1000, clientHeight: 400 });
  scroll.scrollTop = 600; // at bottom
  setGeometry(scroll, { scrollHeight: 1200, clientHeight: 400 }); // content grew
  observerFor(content)!.trigger();
  assert(
    scroll.scrollTop === 1200,
    `pinned viewport snaps to bottom synchronously in the observer callback (got scrollTop=${scroll.scrollTop})`,
  );

  // --- 3. viewport resize (composer growth) re-pins synchronously ---
  scroll.scrollTop = 800;
  setGeometry(scroll, { scrollHeight: 1200, clientHeight: 300 });
  observerFor(scroll)!.trigger();
  assert(
    scroll.scrollTop === 1200,
    `pinned viewport re-pins synchronously when the viewport itself resizes (got scrollTop=${scroll.scrollTop})`,
  );

  // --- 3b. the authority disables native scroll anchoring on its viewport ---
  // (window-shift commits with net-zero height change fire no ResizeObserver,
  // and the browser's own anchoring would drag a pinned viewport off bottom.)
  assert(
    scroll.style.overflowAnchor === "none",
    `scroll authority disables native overflow-anchor (got "${scroll.style.overflowAnchor}")`,
  );

  // --- 3c. a real scroll event is sufficient evidence of user detachment ---
  // Browser/webview input paths do not consistently deliver wheel before
  // scroll, so the viewport must not depend on that precursor.
  scroll.scrollTop = 400;
  await act(async () => {
    hookResult!.handleScroll({ currentTarget: scroll } as unknown as React.UIEvent<HTMLDivElement>);
  });
  await renderFixture(root, false, 2);
  assert(
    scroll.scrollTop === 400,
    `incoming messages preserve a manually detached viewport (got scrollTop=${scroll.scrollTop})`,
  );

  const detachedWrites = countScrollTopWrites(scroll);
  await renderFixture(root, false, 2);
  assert(
    detachedWrites.value() === 0,
    `an unrelated commit does not interrupt detached native momentum (writes=${detachedWrites.value()})`,
  );

  await act(async () => hookResult!.scrollToBottom());

  setGeometry(scroll, { scrollHeight: 1200, clientHeight: 400 });
  scroll.scrollTop = 800;
  const pinnedWrites = countScrollTopWrites(scroll);
  await renderFixture(root, false, 2);
  assert(
    pinnedWrites.value() === 0,
    `an unrelated commit does not reassign an already pinned viewport (writes=${pinnedWrites.value()})`,
  );

  // --- 3d. every pinned commit re-syncs before paint even without a resize ---
  // (window shift: top row dropped + new row appended, total height unchanged.)
  scroll.scrollTop = 700;
  await renderFixture(root, false, 2);
  assert(
    scroll.scrollTop === 1200,
    `commit-time layout sync re-pins without a ResizeObserver delivery (got scrollTop=${scroll.scrollTop})`,
  );

  await act(async () => root.unmount());
  host.remove();
}

// --- 4. detached anchor resolution is synchronous too ---
{
  clearPersistedChatScrollState("tab:sess");
  FakeResizeObserver.instances.length = 0;
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await renderFixture(root, false, 3);

  const content = host.querySelector('[data-testid="content"]') as HTMLElement;
  const scroll = host.querySelector('[data-testid="scroll"]') as HTMLElement;
  setGeometry(scroll, { scrollHeight: 1000, clientHeight: 400 });
  const rows = [...content.querySelectorAll<HTMLElement>("[data-message-id]")];
  assert(rows.length === 3, "fixture rendered three anchor candidates");
  const offsets = new Map<HTMLElement, number>([[rows[0], 0], [rows[1], 100], [rows[2], 300]]);
  for (const row of offsets.keys()) {
    Object.defineProperty(row, "offsetTop", { configurable: true, get: () => offsets.get(row) });
  }

  await act(async () => {
    scroll.scrollTop = 150;
    hookResult!.handleScroll({ currentTarget: scroll } as unknown as React.UIEvent<HTMLDivElement>);
  });

  // History prepend: every row shifts down by 300px.
  for (const row of rows) offsets.set(row, (offsets.get(row) ?? 0) + 300);
  setGeometry(scroll, { scrollHeight: 1300, clientHeight: 400 });
  scroll.scrollTop = 150;
  observerFor(content)!.trigger();
  assert(
    scroll.scrollTop === 450,
    `detached anchor re-resolves synchronously after a prepend (got scrollTop=${scroll.scrollTop})`,
  );

  await act(async () => root.unmount());
  host.remove();
}

flushRaf();
}

main()
  .then(() => console.log("chat scroll observer lifecycle checks passed"))
  .catch((error) => {
    console.error(error);
    process.exit(1);
  });
