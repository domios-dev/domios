import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { clearPersistedChatScrollState } from "./chatViewport";
import { useChatScroll, type UseChatScrollResult } from "./useChatScroll";

const testWindow = new Window({ url: "http://localhost/" });
for (const [key, value] of Object.entries({ window: testWindow, document: testWindow.document,
  navigator: testWindow.navigator, HTMLElement: testWindow.HTMLElement, Event: testWindow.Event, IS_REACT_ACT_ENVIRONMENT: true })) {
  Object.defineProperty(globalThis, key, { configurable: true, value });
}

class TestResizeObserver {
  static observers = new Set<TestResizeObserver>();
  constructor(private callback: () => void) { TestResizeObserver.observers.add(this); }
  observe() {}
  unobserve() {}
  disconnect() { TestResizeObserver.observers.delete(this); }
  static deliver() { for (const observer of this.observers) observer.callback(); }
}
Object.defineProperty(globalThis, "ResizeObserver", { configurable: true, value: TestResizeObserver });

let hook: UseChatScrollResult;
const followTail = { current: true };
function Fixture({ active, revision }: { active: boolean; revision: number }) {
  hook = useChatScroll({ sessionKey: "gesture-wiring", active, loading: false, messageCount: 3,
    contentVersion: revision, prependVersion: "first", followTailRef: followTail });
  return <div ref={hook.scrollRef} onScroll={hook.handleScroll} />;
}

async function main() {
  clearPersistedChatScrollState("gesture-wiring");
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<Fixture active={false} revision={0} />));
  const scroll = host.firstElementChild as HTMLDivElement;
  Object.defineProperty(scroll, "onscrollend", { configurable: true, value: null });
  let nativeTop = 0;
  let height = 1000;
  let writes = 0;
  Object.defineProperties(scroll, {
    scrollHeight: { configurable: true, get: () => height },
    clientHeight: { configurable: true, get: () => 400 },
    scrollTop: { configurable: true, get: () => nativeTop, set: (value: number) => {
      writes++;
      nativeTop = Math.max(0, Math.min(value, height - 400));
    } },
  });
  const fire = (type: string) => scroll.dispatchEvent(new Event(type, { bubbles: true }));
  const repair = () => { hook.handleContentLayout(); TestResizeObserver.deliver(); };
  try {
    await act(async () => root.render(<Fixture active revision={1} />));
    writes = 0;
    await act(async () => repair());
    assert.equal(writes, 0, "already-pinned repairs never invoke the native setter");

    await act(async () => fire("touchstart"));
    nativeTop = 550;
    height = 1200;
    await act(async () => fire("scroll"));
    assert.equal(followTail.current, false, "concurrent incoming content cannot retain pinned authority over a user scroll");
    await act(async () => {
      repair();
      root.render(<Fixture active revision={2} />);
    });
    assert.equal(writes, 0, "content commits and resize repairs defer while the user touches");

    await act(async () => fire("touchend"));
    nativeTop = 500;
    height = 1400;
    await act(async () => { fire("scroll"); repair(); root.render(<Fixture active revision={3} />); });
    assert.equal(writes, 0, "momentum retains ownership after touchend");
    await act(async () => fire("scrollend"));
    assert.equal(followTail.current, false, "gesture completion cannot reattach a reader");

    await act(async () => hook.scrollToBottom());
    assert.equal(writes, 1, "explicit latest performs one necessary write");
    writes = 0;
    await act(async () => fire("touchstart"));
    height = 1600;
    await act(async () => { repair(); root.render(<Fixture active revision={4} />); });
    assert.equal(writes, 0, "arrival before the native scroll event cannot yank an active touch");
    await act(async () => fire("touchend"));
    assert.equal(writes, 1, "a tap without movement releases a deferred pinned repair");
    assert.equal(followTail.current, true);
    await act(async () => root.render(<Fixture active={false} revision={5} />));
    for (let proto: object | null = scroll; proto; proto = Object.getPrototypeOf(proto)) {
      if (Object.getOwnPropertyDescriptor(proto, "onscrollend")?.configurable) Reflect.deleteProperty(proto, "onscrollend");
    }
    assert.equal("onscrollend" in scroll, false);
    await act(async () => root.render(<Fixture active revision={6} />));
    writes = 0;
    await act(async () => fire("touchstart"));
    height = 1800;
    await act(async () => { repair(); root.render(<Fixture active revision={7} />); });
    assert.equal(writes, 0);
    await act(async () => fire("touchend"));
    assert.equal(writes, 1, "without scrollend touchend releases a pinned arrival repair");
    writes = 0;
    height = 2000;
    await act(async () => { repair(); root.render(<Fixture active revision={8} />); });
    assert.equal(writes, 1, "without further native movement a pinned chat keeps following arrivals");
    await act(async () => fire("scroll"));
    await act(async () => fire("touchstart"));
    nativeTop -= 100;
    await act(async () => fire("scroll"));
    await act(async () => fire("touchend"));
    nativeTop = height - 400;
    await act(async () => fire("scroll"));
    assert.equal(followTail.current, true, "without scrollend returning to the bottom resumes following");
    writes = 0;
    height = 2200;
    await act(async () => { repair(); root.render(<Fixture active revision={9} />); });
    assert.equal(writes, 1, "the bottom native event releases momentum suppression");
    writes = 0;
    await act(async () => fire("touchstart"));
    nativeTop -= 100;
    height = 2400;
    await act(async () => { repair(); root.render(<Fixture active revision={10} />); });
    await act(async () => fire("touchend"));
    assert.equal(writes, 0, "without scrollend touchend observes movement before a queued native scroll event");
    assert.equal(followTail.current, false, "the released gesture cannot repair a moved reader back to bottom");
    console.log("7 chat gesture wiring checks passed");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    clearPersistedChatScrollState("gesture-wiring");
    await testWindow.happyDOM.close();
  }
}
void main().catch(error => { console.error(error); process.exitCode = 1; });
