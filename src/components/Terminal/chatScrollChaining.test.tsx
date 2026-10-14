import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { BoundedMessageContent } from "../../../packages/chat-ui/src/BoundedMessageContent";
import { ScrollToLatestButton } from "./ScrollToLatestButton";
import { useChatScroll, type UseChatScrollResult } from "./useChatScroll";
import { clearPersistedChatScrollState } from "./chatViewport";

const testWindow = new Window({ url: "http://localhost/" });
for (const [key, value] of Object.entries({ window: testWindow, document: testWindow.document, navigator: testWindow.navigator,
  HTMLElement: testWindow.HTMLElement, Event: testWindow.Event, IS_REACT_ACT_ENVIRONMENT: true })) {
  Object.defineProperty(globalThis, key, { configurable: true, value });
}
class TestResizeObserver {
  static callbacks = new Set<() => void>();
  constructor(private callback: () => void) { TestResizeObserver.callbacks.add(callback); }
  observe() {}
  unobserve() {}
  disconnect() { TestResizeObserver.callbacks.delete(this.callback); }
  static deliver() { for (const callback of this.callbacks) callback(); }
}
Object.defineProperty(globalThis, "ResizeObserver", { configurable: true, value: TestResizeObserver });

const globals = readFileSync(new URL("../../styles/globals.css", import.meta.url), "utf8");
const outerPolicy = globals.match(/\*\s*\{[^}]*overscroll-behavior:[^}]*\}/)?.[0];
assert(outerPolicy, "load the shipped app overscroll policy");
const css = document.createElement("style");
css.textContent = outerPolicy + readFileSync(new URL("./chatLayout.css", import.meta.url), "utf8");
document.head.append(css);
let hook: UseChatScrollResult;
const follows = { current: true };
function Fixture({ active }: { active: boolean }) {
  hook = useChatScroll({ sessionKey: "chaining-wiring", active, loading: false, messageCount: 1,
    contentVersion: "first", prependVersion: "first", followTailRef: follows });
  return <>
    <div ref={hook.scrollRef} onScroll={hook.handleScroll} role="log" aria-label="Conversation" className="ct-chat-surface">
      <BoundedMessageContent collapsedHeight="256px" role="region" aria-label="Report body">
        <p>{"long report line\n".repeat(100)}</p>
      </BoundedMessageContent>
    </div>
    <ScrollToLatestButton testId="latest" onClick={hook.scrollToBottom} />
  </>;
}
async function main() {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  clearPersistedChatScrollState("chaining-wiring");
  try {
    await act(async () => root.render(<Fixture active={false} />));
    const transcript = host.querySelector<HTMLDivElement>('[role="log"]')!;
    const body = host.querySelector<HTMLDivElement>('[aria-label="Report body"]')!;
    let outerTop = 600;
    let outerHeight = 1000;
    Object.defineProperties(transcript, {
      clientHeight: { get: () => 400 }, scrollHeight: { get: () => outerHeight },
      scrollTop: { get: () => outerTop, set: (value) => { outerTop = Math.max(0, Math.min(value, outerHeight - 400)); } },
      onscrollend: { configurable: true, value: null },
    });
    Object.defineProperties(body, { clientHeight: { get: () => 256 }, scrollHeight: { get: () => 800 } });
    await act(async () => { root.render(<Fixture active />); TestResizeObserver.deliver(); });
    assert.equal(window.getComputedStyle(transcript).overflowY, "auto", "the transcript remains a scrollport");
    assert.equal(window.getComputedStyle(body).getPropertyValue("overscroll-behavior-y"), "auto", "message edges chain to the transcript");
    assert.equal(window.getComputedStyle(transcript).getPropertyValue("overscroll-behavior"), "none", "the outer app boundary remains contained");
    assert.equal(body.style.overflowY, "hidden", "default message bodies cannot consume vertical scrolling");
    const button = Array.from(host.querySelectorAll("button")).find((node) => node.textContent === "Show all");
    assert(button, "a clipped report offers an accessible Show all button");
    assert.equal(button.getAttribute("aria-controls"), body.id);
    const wheel = async (deltaY: number) => {
      const event = new window.WheelEvent("wheel", { bubbles: true, cancelable: true, deltaY });
      await act(async () => body.dispatchEvent(event));
      assert.equal(event.defaultPrevented, false, "native scrolling is never cancelled");
    };
    const repair = async () => { await act(async () => hook.handleContentLayout()); };
    const end = async () => { await act(async () => transcript.dispatchEvent(new Event("scrollend"))); };
    await wheel(-30);
    outerHeight = 1100;
    await repair();
    assert.equal(outerTop, 600, "wheel over the clipped body gives the transcript gesture authority");
    await end();
    await act(async () => button.click());
    assert.equal(button.getAttribute("aria-expanded"), "true");
    assert.equal(body.style.maxHeight, "332.8px");
    assert.equal(body.style.overflowY, "auto");
    body.scrollTop = 200;
    await wheel(-0.5);
    outerHeight = 1200;
    await repair();
    assert.equal(outerTop, 800, "trackpad movement consumed inside an expanded report does not freeze transcript following");
    body.scrollTop = 0;
    await wheel(-30);
    outerHeight = 1300;
    await repair();
    assert.equal(outerTop, 800, "at the inner top the transcript owns upward scrolling");
    await end();
    body.scrollTop = 544;
    outerTop = 700;
    await act(async () => transcript.dispatchEvent(new Event("scroll")));
    await wheel(30);
    outerTop = 800;
    await act(async () => transcript.dispatchEvent(new Event("scroll")));
    assert.equal(follows.current, false, "downward scrolling at the inner bottom moves the detached transcript");
    const latest = host.querySelector<HTMLButtonElement>('button[aria-label="Scroll to latest"]')!;
    await act(async () => latest.click());
    assert.equal(outerTop, 900);
    assert.equal(follows.current, true, "latest resets active wheel ownership and resumes following");
    for (const type of ["touchstart", "touchend", "touchcancel"]) {
      const event = new Event(type, { bubbles: true, cancelable: true });
      await act(async () => body.dispatchEvent(event));
      assert.equal(event.defaultPrevented, false, `${type} leaves native touch scrolling enabled`);
    }
    await act(async () => button.click());
    assert.equal(button.getAttribute("aria-expanded"), "false");
    assert.equal(body.scrollTop, 0, "collapse restores the preview from its beginning");
    assert.equal(body.style.overflowY, "hidden");
    console.log("10 Chat collapse, native chaining and latest wiring checks passed");
  } finally {
    await act(async () => root.unmount());
    clearPersistedChatScrollState("chaining-wiring");
    host.remove();
    await testWindow.happyDOM.close();
  }
}
void main().catch((error) => { console.error(error); process.exitCode = 1; });
