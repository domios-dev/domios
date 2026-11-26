import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { ScrollToLatestButton } from "./ScrollToLatestButton";
import { clearPersistedChatScrollState } from "./chatViewport";
import { useChatScroll, type UseChatScrollResult } from "./useChatScroll";
import { CHAT_TAP_MS } from "./useChatScrollGesture";

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
// The gesture tells a tap from a hold by how long the finger stayed down. The clock only moves when a case says so,
// so a slow machine never turns a tap into a hold.
let clock = 0;
Object.defineProperty(performance, "now", { configurable: true, value: () => clock });

let hook: UseChatScrollResult;
let drift = 0;
let foldLayout = () => {};
let nextFrame = 0;
const frames = new Map<number, FrameRequestCallback>();
Object.defineProperties(testWindow, {
  requestAnimationFrame: { configurable: true, value: (callback: FrameRequestCallback) => { frames.set(++nextFrame, callback); return nextFrame; } },
  cancelAnimationFrame: { configurable: true, value: (id: number) => { frames.delete(id); } },
});
async function frame() {
  await act(async () => {
    const callbacks = [...frames.values()];
    frames.clear();
    callbacks.forEach(callback => callback(0));
  });
}
const followTail = { current: true };
const userScrolling = { current: false };
let newerCount = 0;
let newerReveals = 0;
let jumpTopCalls = 0;
let jumpLatestCalls = 0;
let onNewerRevealHook: (() => void) | null = null;
function Fixture({ active, revision, firstId = "first" }: { active: boolean; revision: number; firstId?: string }) {
  hook = useChatScroll({ sessionKey: "gesture-wiring", active, loading: false, messageCount: 3,
    contentVersion: revision, prependVersion: firstId, followTailRef: followTail, userScrollingRef: userScrolling,
    newerCount,
    onNewerEdge: () => { newerReveals++; onNewerRevealHook?.(); return true; },
    onJumpToTop: () => { jumpTopCalls++; },
    onJumpToLatest: () => { jumpLatestCalls++; newerCount = 0; },
    foldLayoutDrift: () => { foldLayout(); const value = drift; drift = 0; return value; } });
  return <><div ref={hook.scrollRef} tabIndex={-1} onScroll={hook.handleScroll} />
    <ScrollToLatestButton testId="latest" gestureOwned={hook.gestureOwned} userScrollingRef={userScrolling} onClick={hook.scrollToBottom} />
  </>;
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
  let writeBefore = 0;
  Object.defineProperties(scroll, {
    scrollHeight: { configurable: true, get: () => height },
    clientHeight: { configurable: true, get: () => 400 },
    clientWidth: { configurable: true, get: () => 792 },
    offsetWidth: { configurable: true, get: () => 800 },
    offsetHeight: { configurable: true, get: () => 400 },
    clientLeft: { configurable: true, get: () => 0 },
    clientTop: { configurable: true, get: () => 0 },
    getBoundingClientRect: { configurable: true, value: () => ({ left: 0, right: 800, top: 0, bottom: 400, width: 800, height: 400, x: 0, y: 0, toJSON: () => ({}) }) },
    scrollTop: { configurable: true, get: () => nativeTop, set: (value: number) => {
      writes++;
      writeBefore = nativeTop;
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

    await act(async () => hook.scrollToBottom());
    assert.equal(writes, 0, "jump during touch cannot write");
    assert.equal(hook.gestureOwned, true, "jump cannot reset ownership");
    assert.equal(scroll.style.overflowAnchor, "none");
    await act(async () => fire("touchend"));
    nativeTop = 500;
    height = 1400;
    await act(async () => { fire("scroll"); repair(); root.render(<Fixture active revision={3} />); });
    assert.equal(writes, 0, "momentum retains ownership after touchend");
    await act(async () => hook.scrollToBottom());
    assert.equal(writes, 0, "synthetic momentum click cannot write");
    assert.equal(hook.gestureOwned, true, "synthetic momentum click cannot reset ownership");
    const button = host.querySelector<HTMLButtonElement>('button[aria-label="Scroll to latest"]')!;
    await act(async () => button.dispatchEvent(new window.PointerEvent("pointerdown", { bubbles: true })));
    await act(async () => fire("scrollend"));
    await act(async () => button.dispatchEvent(new window.MouseEvent("click", { bubbles: true, detail: 1 })));
    assert.equal(writes, 0, "a click that began during momentum stays rejected after quiescence");
    assert.equal(scroll.style.overflowAnchor, "auto");
    assert.equal(followTail.current, false, "gesture completion cannot reattach a reader");

    await act(async () => button.dispatchEvent(new window.PointerEvent("pointerdown", { bubbles: true })));
    await act(async () => button.dispatchEvent(new window.MouseEvent("click", { bubbles: true, detail: 1 })));
    assert.equal(writes, 1, "idle deliberate latest performs one necessary write");
    writes = 0;
    await act(async () => fire("touchstart"));
    height = 1600;
    await act(async () => { repair(); root.render(<Fixture active revision={4} />); });
    assert.equal(writes, 0, "arrival before the native scroll event cannot yank an active touch");
    await act(async () => fire("touchend"));
    assert.equal(writes, 1, "a tap without movement releases a deferred pinned repair");
    assert.equal(followTail.current, true);
    {
      // A hold, unlike a tap: content arriving under a finger held longer than a tap leaves the view where it is.
      // The case keeps its own geometry and puts the sequence's back afterwards, so later cases see their arrivals.
      const heightBefore = height;
      writes = 0;
      await act(async () => fire("touchstart"));
      clock += CHAT_TAP_MS + 50;
      height += 200;
      await act(async () => { repair(); root.render(<Fixture active revision={41} />); });
      assert.equal(followTail.current, false, "content arriving under a held finger detaches the view");
      await act(async () => fire("touchend"));
      assert.equal(writes, 0, "releasing a hold does not snap to the new bottom");
      assert.equal(followTail.current, false, "the held view stays detached and offers the jump");
      await act(async () => hook.scrollToBottom());
      assert.equal(followTail.current, true, "the jump re-pins the view");
      height = heightBefore;
      nativeTop = heightBefore - 400;
    }
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
    assert.equal(writes, 0, "without scrollend even a tap waits for observed quiescence");
    await frame();
    await frame();
    await frame();
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
    await frame();
    await frame();
    await frame();
    await frame();
    assert.equal(followTail.current, true, "without scrollend returning to the bottom resumes following");
    writes = 0;
    height = 2200;
    await act(async () => { repair(); root.render(<Fixture active revision={9} />); });
    assert.equal(writes, 1, "observed rest at the bottom releases momentum suppression");
    writes = 0;
    await act(async () => fire("touchstart"));
    nativeTop -= 100;
    height = 2400;
    await act(async () => { repair(); root.render(<Fixture active revision={10} />); });
    await act(async () => fire("touchend"));
    assert.equal(writes, 0, "without scrollend touchend observes movement before a queued native scroll event");
    await act(async () => fire("scroll"));
    assert.equal(followTail.current, false, "the released gesture cannot repair a moved reader back to bottom");
    drift = 50;
    foldLayout = () => { if (drift !== 0 && scroll.style.overflowAnchor !== "none") nativeTop += drift; };
    for (let index = 0; index < 4; index++) {
      nativeTop -= 10;
      await act(async () => { fire("scroll"); repair(); });
      await act(async () => hook.scrollToBottom());
      await frame();
      assert.equal(hook.gestureOwned, true);
      assert.equal(drift, 50, "momentum never folds a held layout deviation");
      assert.equal(writes, 0);
    }
    await frame();
    await frame();
    assert.equal(writes, 0, "two unchanged animation frames cannot release momentum");
    const lastOwnedTop = nativeTop;
    await frame();
    assert.equal(drift, 0);
    assert.equal(writes, 1, "quiescence folds the held deviation in exactly one write");
    assert.equal(writeBefore, lastOwnedTop, "fold starts at the last owned position before native anchoring resumes");
    await frame();
    assert.equal(writes, 1);

    // Re-enable scrollend and prove scrollbar drag, keyboard scroll, and manual top-load prepends.
    Object.defineProperty(scroll, "onscrollend", { configurable: true, value: null });
    await act(async () => root.render(<Fixture active={false} revision={11} />));
    await act(async () => root.render(<Fixture active revision={12} />));
    await act(async () => hook.scrollToBottom());
    assert.equal(followTail.current, true);
    writes = 0;

    // 1. Scrollbar thumb drag: owns immediately, never writes or pins mid-drag (even touching bottom), settles on scrollend.
    await act(async () => scroll.dispatchEvent(new window.PointerEvent("pointerdown", {
      bubbles: true, button: 0, pointerType: "mouse", clientX: 796, clientY: 380,
    })));
    assert.equal(hook.gestureOwned, true, "scrollbar pointerdown owns the gesture immediately");
    nativeTop = 0;
    await act(async () => { fire("scroll"); repair(); root.render(<Fixture active revision={13} />); });
    assert.equal(followTail.current, false, "dragging up detaches");
    assert.equal(writes, 0, "no app scroll writes during scrollbar drag");
    nativeTop = height - 400;
    await act(async () => { fire("scroll"); repair(); });
    assert.equal(followTail.current, false, "touching the bottom mid-drag never pins or resets the window");
    assert.equal(writes, 0, "still zero app writes while dragging");
    nativeTop = 100;
    await act(async () => { fire("scroll"); });
    await act(async () => fire("scrollend"));
    assert.equal(hook.gestureOwned, false, "Chromium scrollbar release scrollend settles ownership");
    assert.equal(followTail.current, false, "settled mid-history drag stays detached");

    // 2. Keyboard scroll (PageUp): owns until scrollend and blocks app writes.
    writes = 0;
    await act(async () => scroll.dispatchEvent(new window.KeyboardEvent("keydown", { bubbles: true, code: "PageUp" })));
    assert.equal(hook.gestureOwned, true, "PageUp with room to scroll owns the gesture");
    nativeTop = 0;
    height += 200;
    await act(async () => { fire("scroll"); repair(); root.render(<Fixture active revision={14} />); });
    assert.equal(writes, 0, "keyboard scroll blocks app writes until scrollend");
    await act(async () => fire("scrollend"));
    assert.equal(hook.gestureOwned, false, "keyboard scroll settles on scrollend");

    // 3. Manual Load earlier ("top" prepend): keeps scrollTop at 0 and suppresses newer-row bounce past the 300-row cap.
    nativeTop = height - 400;
    newerCount = 0;
    newerReveals = 0;
    await act(async () => hook.prepareForHistoryPrepend("top"));
    newerCount = 100;
    height += 1200;
    await act(async () => root.render(<Fixture active revision={15} firstId="older-1" />));
    assert.equal(nativeTop, 0, "manual Load earlier keeps the focused top control in view");
    assert.equal(newerReveals, 0, "sliding the window older never bounces back via revealNewer");
    assert.equal(followTail.current, false);

    // Repeating past the reading cap continues sliding older without bouncing.
    await act(async () => hook.prepareForHistoryPrepend("top"));
    newerCount = 200;
    await act(async () => root.render(<Fixture active revision={16} firstId="older-2" />));
    assert.equal(nativeTop, 0);
    assert.equal(newerReveals, 0);

    // 4. Keyboard PageDown from mid-history: blocks writes and newer-page reveal until after scrollend completes.
    nativeTop = 400;
    newerCount = 100;
    newerReveals = 0;
    writes = 0;
    await act(async () => root.render(<Fixture active revision={17} firstId="older-2" />));
    await act(async () => scroll.dispatchEvent(new window.KeyboardEvent("keydown", { bubbles: true, code: "PageDown" })));
    assert.equal(hook.gestureOwned, true, "PageDown with room to move owns the gesture");
    nativeTop = height - 400;
    await act(async () => { fire("scroll"); repair(); root.render(<Fixture active revision={18} firstId="older-2" />); });
    assert.equal(writes, 0, "PageDown blocks app writes until scrollend");
    assert.equal(newerReveals, 0, "PageDown defers newer-page loads until scrollend settles");
    const settleOrder: string[] = [];
    const observeScrollEnd = () => settleOrder.push("scrollend");
    scroll.addEventListener("scrollend", observeScrollEnd);
    onNewerRevealHook = () => settleOrder.push("reveal-newer");
    try {
      await act(async () => fire("scrollend"));
      assert.equal(hook.gestureOwned, false, "PageDown settles on scrollend");
      assert.equal(newerReveals, 1, "settled PageDown at bottom reveals held-back newer rows once");
      assert.deepEqual(settleOrder, ["scrollend", "reveal-newer"], "newer-page reveal lands strictly after scrollend");
    } finally {
      scroll.removeEventListener("scrollend", observeScrollEnd);
      onNewerRevealHook = null;
    }

    // 5. Keyboard Home and End jump instantaneously without smooth-scroll animation; End reaches latest in one press.
    nativeTop = 600;
    jumpTopCalls = 0;
    jumpLatestCalls = 0;
    scroll.focus();
    const homeEvent = new window.KeyboardEvent("keydown", { bubbles: true, cancelable: true, code: "Home" });
    await act(async () => scroll.dispatchEvent(homeEvent));
    assert.equal(homeEvent.defaultPrevented, true, "Home prevents browser smooth scroll");
    assert.equal(jumpTopCalls, 1, "Home synchronously prepares the top mounted window");
    assert.equal(nativeTop, 0, "Home jumps instantaneously to scrollTop 0");
    assert.equal(hook.gestureOwned, false, "instant Home leaves no lingering gesture ownership");
    assert.equal(followTail.current, false, "Home detaches from tail");
    assert.equal(document.activeElement, scroll, "Home preserves focus on the log");

    newerCount = 400;
    await act(async () => root.render(<Fixture active revision={19} firstId="older-2" />));
    const endEvent = new window.KeyboardEvent("keydown", { bubbles: true, cancelable: true, code: "End" });
    await act(async () => scroll.dispatchEvent(endEvent));
    assert.equal(endEvent.defaultPrevented, true, "End prevents browser smooth scroll");
    assert.equal(jumpLatestCalls, 1, "End jumps directly to the latest message in one press");
    assert.equal(nativeTop, height - 400, "End lands at the bottom of the latest page");
    assert.equal(followTail.current, true, "End re-pins to the latest message");
    assert.equal(hook.gestureOwned, false, "instant End leaves no lingering gesture ownership");
    assert.equal(document.activeElement, scroll, "End preserves focus on the log");

    // End also jumps to latest when already resting at the bottom edge of an older logical page (newerCount > 0).
    nativeTop = height - 400;
    newerCount = 300;
    followTail.current = false;
    await act(async () => {
      nativeTop = height - 500;
      fire("scroll");
      nativeTop = height - 400;
      root.render(<Fixture active revision={20} firstId="older-2" />);
    });
    const endAtPageEdge = new window.KeyboardEvent("keydown", { bubbles: true, cancelable: true, code: "End" });
    await act(async () => scroll.dispatchEvent(endAtPageEdge));
    assert.equal(endAtPageEdge.defaultPrevented, true, "End at bottom of an older page still jumps to latest");
    assert.equal(jumpLatestCalls, 2, "End from an older page bottom reaches latest in one press");
    assert.equal(followTail.current, true);

    console.log("14 chat gesture wiring checks passed");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    clearPersistedChatScrollState("gesture-wiring");
    await testWindow.happyDOM.close();
  }
}
void main().catch(error => { console.error(error); process.exitCode = 1; });
