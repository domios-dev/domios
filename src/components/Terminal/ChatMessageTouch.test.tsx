import { act, useRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { MessageBubble } from "@codeterm/chat-ui";
import { DOMIOS_TEXT_MIME } from "@codeterm/chat-core";
import { ChatMessageRow } from "./ChatMessageRow";
import { MessageSelectionBar } from "./MessageSelectionBar";
import { useChatMessageSelection } from "./useChatMessageSelection";
import { composerInsertionStore } from "./composerInsertion";
import { composerReplyStore } from "./composerReply";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
vi.mock("../../lib/clientTabUiMode", () => ({ getCurrentTabUiMode: () => "chat", setClientTabUiMode: vi.fn() }));
const messages = [{ id: "touch-one", type: "assistant" as const, content: "First.\n\nSecond **part**." }, { id: "touch-two", type: "assistant" as const, content: "Third.\n\nFourth." }];
function Harness() {
  const surfaceRef = useRef<HTMLDivElement>(null);
  const selection = useChatMessageSelection({ tabId: "touch", agent: "Fermi", surfaceRef, rows: messages.map((msg) => ({ msg, item: { kind: "message", message: msg } })) });
  return <div ref={surfaceRef} role="log" onKeyDown={selection.onKeyDown}>
    {messages.map((msg) => <ChatMessageRow key={msg.id} msg={msg} ariaLabel={msg.id} selected={selection.selected.has(msg.id)} joinNext={false} selecting={selection.active} cursor={!selection.cursor} handlers={selection.rowHandlers}><MessageBubble message={msg} /></ChatMessageRow>)}
    <MessageSelectionBar count={selection.count} partial={selection.partial} onCopy={selection.copySelection} onQuote={selection.quoteSelection} onAsk={selection.askSelection} onClear={selection.clear} />
  </div>;
}
let root: Root | undefined;
let host: HTMLDivElement;
let written: Record<string, string>;
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove(); vi.useRealTimers(); vi.restoreAllMocks();
  composerInsertionStore.clear(); composerReplyStore.clear();
});
async function render() {
  vi.useFakeTimers(); written = {};
  document.execCommand = vi.fn(() => {
    document.dispatchEvent(Object.assign(new Event("copy", { bubbles: true, cancelable: true }), { clipboardData: { setData: (mime: string, value: string) => { written[mime] = value; } } }));
    return true;
  });
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  await act(async () => root!.render(<Harness />));
}
const parts = () => Array.from(host.querySelectorAll<HTMLElement>("[data-message-part-id]"));
const item = (name: string) => document.querySelector<HTMLButtonElement>(`[role='menuitem'][aria-label='${name}']`)!;
const menu = () => document.querySelector("[role='menu']");
async function pointer(target: EventTarget, type: string, x = 20, y = 20) {
  const event = new PointerEvent(type, { bubbles: true, cancelable: true, pointerType: "touch", pointerId: 1, isPrimary: true, clientX: x, clientY: y });
  await act(async () => { target.dispatchEvent(event); });
  return event;
}
async function hold(index = 1) {
  await pointer(parts()[index], "pointerdown");
  await act(async () => vi.advanceTimersByTime(450));
  await pointer(parts()[index], "pointerup");
  const partScope = Array.from(document.querySelectorAll<HTMLButtonElement>("[role='menuitemradio']")).find((button) => button.textContent === "This part")!;
  await act(async () => partScope.click());
}
it("opens the touched part with a preview and copies only its source", async () => {
  await render();
  await pointer(parts()[1], "pointerdown");
  await act(async () => vi.advanceTimersByTime(449)); expect(menu()).toBeNull();
  await act(async () => vi.advanceTimersByTime(1));
  await pointer(parts()[1], "pointerup");
  expect(document.querySelector('[aria-label="Action preview"]')?.textContent).toBe("Second part.");
  await act(async () => item("Copy").click());
  expect(written[DOMIOS_TEXT_MIME]).toBe("Second **part**.");
});
it("asks about only the held part as a quote", async () => {
  await render(); await hold(); await act(async () => item("Ask about this").click());
  expect(composerReplyStore.take("touch")?.content).toBe("Fermi:\n> Second **part**.");
  expect(composerInsertionStore.take("touch")).toEqual([]);
});
it("keeps the sheet open after release, focus loss and compatibility events until a later outside tap", async () => {
  await render();
  await pointer(parts()[1], "pointerdown");
  await act(async () => vi.advanceTimersByTime(450));
  expect(menu()).not.toBeNull();
  await pointer(parts()[1], "pointerup");
  await act(async () => {
    (document.activeElement as HTMLElement).blur();
    document.body.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true, detail: 1 }));
    document.body.dispatchEvent(new MouseEvent("mouseup", { bubbles: true, cancelable: true, detail: 1 }));
    document.body.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, detail: 1 }));
  });
  expect(menu()).not.toBeNull();
  await pointer(document.body, "pointerdown");
  await pointer(document.body, "pointerup");
  expect(menu()).toBeNull();
});
it("consumes the opening compatibility click even when the sheet covers the finger", async () => {
  await render(); await hold();
  const copy = item("Copy");
  await act(async () => {
    copy.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true, detail: 1 }));
    copy.dispatchEvent(new MouseEvent("mouseup", { bubbles: true, cancelable: true, detail: 1 }));
    copy.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, detail: 1 }));
  });
  expect(written).toEqual({});
  expect(menu()).not.toBeNull();
  await pointer(copy, "pointerdown");
  await pointer(copy, "pointerup");
  await act(async () => copy.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, detail: 1 })));
  expect(written[DOMIOS_TEXT_MIME]).toBe("Second **part**.");
});
it("closes with Escape after release has moved focus out of the sheet", async () => {
  await render(); await hold();
  await act(async () => (document.activeElement as HTMLElement).blur());
  await act(async () => document.body.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, code: "Escape" })));
  expect(menu()).toBeNull();
});
it("switches the touch preview and payload to the whole message", async () => {
  await render(); await hold();
  const whole = Array.from(document.querySelectorAll<HTMLButtonElement>("[role='menuitemradio']")).find((button) => button.textContent === "Whole message")!;
  await act(async () => whole.click());
  expect(document.querySelector('[aria-label="Action preview"]')?.textContent).toBe("First.\n\nSecond part.");
  await act(async () => item("Copy").click());
  expect(written[DOMIOS_TEXT_MIME]).toBe("First.\n\nSecond **part**.");
});
it("owns the callout only while holding and dismisses on an outside press", async () => {
  await render(); const context = () => {
    const event = new MouseEvent("contextmenu", { bubbles: true, cancelable: true });
    parts()[1].dispatchEvent(event); return event.defaultPrevented;
  };
  expect(context()).toBe(false);
  await pointer(parts()[1], "pointerdown"); expect(context()).toBe(true);
  await act(async () => vi.advanceTimersByTime(450));
  await pointer(parts()[1], "pointerup"); expect(context()).toBe(false);
  await pointer(document.body, "pointerdown"); expect(menu()).toBeNull();
});
it.each(["tap", "move", "scroll", "pointercancel"])("keeps %s from opening the menu", async (reason) => {
  await render(); const down = await pointer(parts()[1], "pointerdown"); expect(down.defaultPrevented).toBe(false);
  if (reason === "tap") await pointer(parts()[1], "pointerup");
  if (reason === "move") await pointer(parts()[1], "pointermove", 29);
  if (reason === "scroll") await act(async () => host.dispatchEvent(new Event("scroll")));
  if (reason === "pointercancel") await pointer(parts()[1], "pointercancel");
  await act(async () => vi.advanceTimersByTime(450)); expect(menu()).toBeNull();
});
it("cancels a hold on unmount", async () => {
  await render(); await pointer(parts()[1], "pointerdown");
  await act(async () => root!.unmount()); root = undefined;
  await act(async () => vi.advanceTimersByTime(450)); expect(menu()).toBeNull();
});
it("toggles a part once on tap and ignores its compatibility mouse event", async () => {
  await render(); await hold(0); await act(async () => item("Select").click());
  await pointer(parts()[1], "pointerdown"); await pointer(parts()[1], "pointerup");
  await act(async () => {
    parts()[1].dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0 }));
    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    parts()[1].dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
  expect(host.querySelector('[role="toolbar"]')?.textContent).toContain("1 message");
  await pointer(parts()[1], "pointerdown"); await pointer(parts()[1], "pointerup");
  expect(host.querySelector('[role="toolbar"]')?.textContent).toContain("1 part");
});
it("selects a range across messages from the gutter", async () => {
  await render(); await hold(1); await act(async () => item("Select").click());
  const gutter = parts()[0].querySelector<HTMLElement>("[data-message-gutter]")!;
  vi.spyOn(document, "elementFromPoint").mockReturnValue(parts()[2]);
  await pointer(gutter, "pointerdown"); await pointer(document, "pointermove", 20, 120); await pointer(document, "pointerup", 20, 120);
  expect(host.querySelector('[role="toolbar"]')?.textContent).toContain("3 parts");
});
it("allows content scrolling in block mode without toggling a part", async () => {
  await render(); await hold(0); await act(async () => item("Select").click());
  await pointer(parts()[1], "pointerdown");
  const move = await pointer(parts()[1], "pointermove", 20, 40);
  expect(move.defaultPrevented).toBe(false);
  await pointer(parts()[1], "pointerup", 20, 40);
  expect(host.querySelector('[role="toolbar"]')?.textContent).toContain("1 part");
});
