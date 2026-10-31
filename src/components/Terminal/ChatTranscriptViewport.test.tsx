import { act, useMemo, useRef, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { ToolCallBubble } from "../../../packages/chat-ui/src/ToolCallBubble";
import type { PendingPrompt } from "../../lib/types";
import { ChatTranscriptViewport } from "./ChatTranscriptViewport";
import { clearPersistedChatScrollState } from "./chatViewport";
import { useMountedChatWindow } from "./useMountedChatWindow";
import { useChatScroll } from "./useChatScroll";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const SESSION = "startup-prompt-geometry";
const VIEWPORT = 500;
const rows = Array.from({ length: 80 }, (_, index) => ({ msg: { id: `m${index}` } }));
const prompt: PendingPrompt = {
  question: Array.from({ length: 120 }, (_, index) => `opaque-command-${index}`).join("; "),
  options: [{ label: "Once", value: "opaque-once" }, { label: "Always", value: "opaque-always" }, { label: "Reject", value: "opaque-reject" }],
};
let host: HTMLDivElement;
let root: Root;
let frames: Map<number, FrameRequestCallback>;
let writes: number;
let followTail: { current: boolean };
let responses: string[];

function extent(element: HTMLElement): number {
  if (element.dataset.height) return Number(element.dataset.height);
  if (element.dataset.testid === "tab-pending-prompt") return 80 + Math.ceil((element.textContent?.length ?? 0) / 60) * 20;
  if (element.style.height) return Number.parseFloat(element.style.height) || 0;
  return Array.from(element.children).reduce((sum, child) => sum + extent(child as HTMLElement), 0);
}

function scroller(): HTMLDivElement { return host.querySelector('[role="log"]')!; }
function offset(element: HTMLElement): number {
  const parent = element.parentElement;
  if (!parent || element === scroller()) return 0;
  let top = offset(parent);
  for (const sibling of Array.from(parent.children)) {
    if (sibling === element) break;
    top += extent(sibling as HTMLElement);
  }
  return top;
}

function Harness({ pending }: { pending: boolean }) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const [expanded, setExpanded] = useState(false);
  const mount = useMountedChatWindow({ rows, scrollRef, active: true, followTailRef: followTail, sessionKey: SESSION });
  const contentVersion = useMemo(() => ({ rows, revision: mount.revision }), [mount.revision]);
  const scroll = useChatScroll({ scrollRef, sessionKey: SESSION, active: true, loading: false, messageCount: rows.length,
    contentVersion, prependVersion: rows[0].msg.id, followTailRef: followTail, onViewportChange: mount.syncViewport });
  return <ChatTranscriptViewport prompt={pending ? { id: "approval", prompt } : null} bottomInset={0} onRespond={value => responses.push(value)}>
    <div role="log" onScroll={scroll.handleScroll} ref={element => {
      scrollRef.current = element;
      if (!element || Object.prototype.hasOwnProperty.call(element, "scrollHeight")) return;
      let top = 0;
      const maximum = () => Math.max(0, extent(element) - VIEWPORT);
      Object.defineProperties(element, {
        clientHeight: { configurable: true, get: () => VIEWPORT },
        scrollHeight: { configurable: true, get: () => extent(element) },
        scrollTop: { configurable: true, get: () => Math.min(top, maximum()), set: (value: number) => {
          const next = Math.max(0, Math.min(value, maximum()));
          if (next !== top) writes++;
          top = next;
        } },
      });
    }}>
      <div>
        <div ref={mount.topSpacerRef} style={{ height: mount.topSpacer }} />
        <div ref={mount.rowsRef}>
          {rows.slice(mount.range.start, mount.range.end).map(({ msg }) => <div key={msg.id} data-chat-window-row={msg.id}
            data-height={msg.id === "m79" && expanded ? 220 : 100}>
            <div data-message-id={msg.id}>
              {msg.id === "m79" ? <ToolCallBubble expanded={expanded} onToggle={setExpanded} message={{
                id: msg.id, type: "tool_call", content: "", toolKind: "command", toolStatus: "running",
                toolInput: { command: "opaque-command" }, toolDetail: "opaque-command",
              }} /> : msg.id}
            </div>
          </div>)}
        </div>
        <div style={{ height: mount.range.bottom }} />
      </div>
    </div>
  </ChatTranscriptViewport>;
}

async function render(pending: boolean) { await act(async () => root.render(<Harness pending={pending} />)); }
async function flushFrames() {
  await act(async () => {
    while (frames.size) {
      const callbacks = [...frames.values()];
      frames.clear();
      callbacks.forEach(callback => callback(0));
    }
  });
}

beforeEach(() => {
  writes = 0;
  responses = [];
  frames = new Map();
  followTail = { current: true };
  clearPersistedChatScrollState(SESSION);
  let nextFrame = 0;
  vi.stubGlobal("ResizeObserver", undefined);
  vi.spyOn(window, "requestAnimationFrame").mockImplementation(callback => { frames.set(++nextFrame, callback); return nextFrame; });
  vi.spyOn(window, "cancelAnimationFrame").mockImplementation(id => { frames.delete(id); });
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function(this: HTMLElement) {
    const top = this === scroller() ? 0 : offset(this) - scroller().scrollTop;
    const height = this === scroller() ? VIEWPORT : extent(this);
    return { top, bottom: top + height, height, left: 0, right: 800, width: 800, x: 0, y: top, toJSON: () => ({}) };
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  clearPersistedChatScrollState(SESSION);
});

it.each([false, true])("keeps command identity, expansion and transcript geometry through approval startup (detached=%s)", async detached => {
  await render(false);
  const toggle = host.querySelector<HTMLButtonElement>('button[aria-expanded]')!;
  await act(async () => toggle.click());
  expect(toggle.getAttribute("aria-expanded")).toBe("true");
  if (detached) {
    await act(async () => scroller().dispatchEvent(new WheelEvent("wheel", { deltaY: -100, bubbles: true })));
    await act(async () => {
      scroller().scrollTop -= 100;
      scroller().dispatchEvent(new Event("scroll", { bubbles: true }));
      scroller().dispatchEvent(new Event("scrollend"));
    });
    await flushFrames();
  }
  expect(followTail.current).toBe(!detached);
  expect(host.querySelectorAll("[data-chat-window-row]").length).toBeLessThan(rows.length);
  const command = host.querySelector<HTMLElement>('[data-message-id="m79"]')!;
  const before = { top: scroller().scrollTop, height: scroller().scrollHeight, rect: command.getBoundingClientRect().top };
  writes = 0;
  for (const pending of [true, false, true, false]) {
    await render(pending);
    await flushFrames();
    expect(scroller().scrollTop).toBe(before.top);
    expect(scroller().scrollHeight).toBe(before.height);
    expect(command.getBoundingClientRect().top).toBe(before.rect);
    expect(host.querySelector('[data-message-id="m79"]')).toBe(command);
    expect(host.querySelector('button[aria-expanded]')).toBe(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(followTail.current).toBe(!detached);
    if (pending) {
      const approval = host.querySelector('[data-testid="tab-pending-prompt"]')!;
      expect(approval.textContent).toContain(prompt.question);
      expect(scroller().contains(approval)).toBe(false);
      const buttons = approval.querySelectorAll<HTMLButtonElement>("button");
      expect(buttons).toHaveLength(3);
      await act(async () => buttons.forEach(button => button.click()));
    }
  }
  expect(writes).toBe(0);
  expect(responses).toEqual([...prompt.options, ...prompt.options].map(option => option.value));
});
