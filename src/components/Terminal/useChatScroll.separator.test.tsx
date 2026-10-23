import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { clearPersistedChatScrollState } from "./chatViewport";
import { useChatScroll, type UseChatScrollResult } from "./useChatScroll";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const SESSION = "tab-separator:session";
const SEPARATOR_HEIGHT = 28;
const BODY_HEIGHT = 50;
const VIEWPORT = 400;

interface Row { id: string; separator: boolean }

class InertResizeObserver {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}

const followTail = { current: true };
let hook: UseChatScrollResult | null = null;
let root: Root | null = null;
let host: HTMLElement | null = null;

function Harness({ rows, active }: { rows: Row[]; active: boolean }) {
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
      {rows.map((row) => (
        <div key={row.id} data-chat-window-row={row.id}>
          {row.separator && <div role="separator" data-h={SEPARATOR_HEIGHT} />}
          <div data-message-id={row.id} data-h={BODY_HEIGHT}>{row.id}</div>
        </div>
      ))}
    </div>
  );
}

function scroll(): HTMLElement {
  const el = host?.querySelector<HTMLElement>('[data-testid="scroll"]');
  if (!el) throw new Error("chat scroller is not mounted");
  return el;
}

function leaves(): HTMLElement[] {
  return Array.from(scroll().querySelectorAll<HTMLElement>("[data-h]"));
}

function contentHeight(): number {
  return leaves().reduce((sum, leaf) => sum + Number(leaf.dataset.h), 0);
}

function layoutBox(element: HTMLElement): { top: number; height: number } {
  const all = leaves();
  const inside = all.filter((leaf) => element === leaf || element.contains(leaf));
  if (!inside.length) return { top: 0, height: 0 };
  const before = all.slice(0, all.indexOf(inside[0]));
  return {
    top: before.reduce((sum, leaf) => sum + Number(leaf.dataset.h), 0),
    height: inside.reduce((sum, leaf) => sum + Number(leaf.dataset.h), 0),
  };
}

function stubScrollerGeometry(el: HTMLElement): void {
  let top = 0;
  const maxTop = () => Math.max(0, contentHeight() - VIEWPORT);
  Object.defineProperty(el, "clientHeight", { configurable: true, get: () => VIEWPORT });
  Object.defineProperty(el, "scrollHeight", { configurable: true, get: () => Math.max(VIEWPORT, contentHeight()) });
  Object.defineProperty(el, "scrollTop", {
    configurable: true,
    get: () => (top = Math.min(top, maxTop())),
    set: (value: number) => { top = Math.max(0, Math.min(value, maxTop())); },
  });
}

/** Viewport-relative top of every message body that intersects the viewport. */
function visibleBodies(): Map<string, number> {
  const el = scroll();
  const visible = new Map<string, number>();
  for (const body of Array.from(el.querySelectorAll<HTMLElement>("[data-message-id]"))) {
    const top = body.getBoundingClientRect().top;
    if (top + BODY_HEIGHT > 0 && top < VIEWPORT) visible.set(body.dataset.messageId!, top);
  }
  return visible;
}

function page(prefix: string, count: number): Row[] {
  return Array.from({ length: count }, (_, index) => ({ id: `${prefix}-${index}`, separator: index === 0 }));
}

async function render(rows: Row[], active = true): Promise<void> {
  await act(async () => root!.render(<Harness rows={rows} active={active} />));
}

let rectDescriptor: PropertyDescriptor | undefined;

beforeAll(() => {
  rectDescriptor = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "getBoundingClientRect");
  Object.defineProperty(HTMLElement.prototype, "getBoundingClientRect", { configurable: true, value(this: HTMLElement) {
    const scroller = this.closest<HTMLElement>('[data-testid="scroll"]');
    const box = scroller && scroller !== this ? layoutBox(this) : { top: 0, height: VIEWPORT };
    const top = scroller && scroller !== this ? box.top - scroller.scrollTop : 0;
    return { top, bottom: top + box.height, height: box.height, left: 0, right: 800, width: 800, x: 0, y: top, toJSON: () => ({}) };
  } });
  vi.stubGlobal("ResizeObserver", InertResizeObserver);
});

afterAll(() => {
  if (rectDescriptor) Object.defineProperty(HTMLElement.prototype, "getBoundingClientRect", rectDescriptor);
  vi.unstubAllGlobals();
});

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  host?.remove();
  root = null;
  host = null;
  hook = null;
  followTail.current = true;
  clearPersistedChatScrollState(SESSION);
});

describe("older-history prepend across a day separator", () => {
  it("moves no visible message when the separator leaves the first row for the prepended page", async () => {
    const current = page("today", 20);
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    await render(current, false);
    stubScrollerGeometry(scroll());
    await render(current);

    scroll().scrollTop = 10;
    await act(async () => hook!.handleScroll({ currentTarget: scroll() } as never));
    expect(followTail.current).toBe(false);
    const before = visibleBodies();
    expect(before.get("today-0")).toBe(SEPARATOR_HEIGHT - 10);

    await act(async () => hook!.prepareForHistoryPrepend());
    const older = page("today-older", 8);
    await render([...older, { ...current[0], separator: false }, ...current.slice(1)]);

    const after = visibleBodies();
    for (const [id, top] of before) expect(after.get(id), id).toBe(top);
  });
});
