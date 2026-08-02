import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { clearPersistedChatScrollState } from "./chatViewport";
import { useChatScroll, type UseChatScrollResult } from "./useChatScroll";

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
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
  return createElement("div", { ref: hookResult.scrollRef, "data-testid": "scroll" });
}

async function render(root: Root, revision: number, messageCount: number, firstId = "m-1"): Promise<void> {
  await act(async () => root.render(createElement(Fixture, { revision, messageCount, firstId })));
}

async function main(): Promise<void> {
  clearPersistedChatScrollState("tab:session");
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await render(root, 0, 0);
  const scroll = host.querySelector('[data-testid="scroll"]') as HTMLElement;
  setGeometry(scroll, { scrollHeight: 1000, clientHeight: 400 });
  await render(root, 1, 1);
  assert(scroll.scrollTop === 600, "initial pinned transcript opens at latest");
  assert(scroll.style.overflowAnchor === "none", "native anchoring cannot become a second writer");

  scroll.scrollTop = 599.9;
  await act(async () => hookResult!.handleScroll({ currentTarget: scroll } as never));
  assert(!followTail.current, "the first upward movement detaches despite bottom rounding tolerance");

  setGeometry(scroll, { scrollHeight: 1200, clientHeight: 400 });
  await render(root, 2, 2);
  assert(scroll.scrollTop === 599.9, "new canonical content never moves a detached viewport");

  await act(async () => hookResult!.prepareForHistoryPrepend());
  setGeometry(scroll, { scrollHeight: 1250, clientHeight: 400 });
  await render(root, 3, 3);
  assert(scroll.scrollTop === 599.9, "an intervening live update does not consume or move the prepend snapshot");
  setGeometry(scroll, { scrollHeight: 1550, clientHeight: 400 });
  await render(root, 4, 4, "m-0");
  assert(scroll.scrollTop === 899.9, "older-history prepend preserves the visible position");

  await act(async () => hookResult!.scrollToBottom());
  assert(scroll.scrollTop === 1150, "explicit down returns to latest");
  setGeometry(scroll, { scrollHeight: 1750, clientHeight: 400 });
  await render(root, 5, 5, "m-0");
  assert(scroll.scrollTop === 1350, "pinned viewport follows new canonical content");

  await act(async () => root.unmount());
  host.remove();
  console.log("chat scroll authority checks passed");
}

void main().catch((error) => {
  console.error(error);
  process.exit(1);
});
