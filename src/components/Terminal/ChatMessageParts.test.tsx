import { act, useRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { MessageBubble } from "@codeterm/chat-ui";
import { DOMIOS_TEXT_MIME, type NormalizedChatMessage } from "@codeterm/chat-core";
import { ChatMessageRow } from "./ChatMessageRow";
import { MessageSelectionBar } from "./MessageSelectionBar";
import { useChatMessageSelection } from "./useChatMessageSelection";
import { composerReplyStore } from "./composerReply";
import { composerInsertionStore } from "./composerInsertion";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
vi.mock("../../lib/clientTabUiMode", () => ({ getCurrentTabUiMode: () => "chat", setClientTabUiMode: vi.fn() }));
const SOURCE = "# Opening\n\nFirst **paragraph**.\n\nSecond _paragraph_.\n\nLast paragraph.";
const messages: NormalizedChatMessage[] = [
  { id: "one", type: "assistant", content: SOURCE },
  { id: "two", type: "assistant", content: "Next message.\n\nFinal part." },
];
function Harness({ data = messages }: { data?: NormalizedChatMessage[] }) {
  const surfaceRef = useRef<HTMLDivElement>(null);
  const selection = useChatMessageSelection({ tabId: "parts", agent: "Fermi", surfaceRef, rows: data.map((msg) => ({ msg, item: { kind: "message", message: msg } })) });
  return <div ref={surfaceRef} role="log" onKeyDown={selection.onKeyDown}>
    {data.map((msg) => <ChatMessageRow key={msg.id} msg={msg} ariaLabel={msg.id} selected={selection.selected.has(msg.id)} joinNext={false}
      selecting={selection.active} cursor={!selection.cursor} handlers={selection.rowHandlers}>
      <MessageBubble message={msg} />
    </ChatMessageRow>)}
    <MessageSelectionBar count={selection.count} partial={selection.partial} onCopy={selection.copySelection} onQuote={selection.quoteSelection} onAsk={selection.askSelection} onClear={selection.clear} />
  </div>;
}
let root: Root | undefined;
let host: HTMLDivElement;
let written: Record<string, string> = {};
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  composerReplyStore.clear(); composerInsertionStore.clear();
});
async function render(data = messages) {
  written = {};
  document.execCommand = vi.fn(() => {
    document.dispatchEvent(Object.assign(new Event("copy", { bubbles: true, cancelable: true }), {
      clipboardData: { setData: (mime: string, text: string) => { written[mime] = text; } },
    }));
    return true;
  });
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  await act(async () => root!.render(<Harness data={data} />));
  Array.from(host.querySelectorAll<HTMLElement>("[data-message-part-id]")).forEach((element, index) => {
    element.getBoundingClientRect = () => ({ top: index * 100, bottom: index * 100 + 80, height: 80, left: 0, right: 500, width: 500, x: 0, y: index * 100, toJSON: () => ({}) });
  });
  await scope(false);
}
const parts = () => Array.from(host.querySelectorAll<HTMLElement>("[data-message-part-id]"));
const menuItem = (label: string) => host.querySelector<HTMLButtonElement>(`[role="menuitem"][aria-label="${label}"]`)!;
const toolbar = () => host.querySelector('[role="toolbar"]');
async function scope(whole: boolean) {
  const control = Array.from(host.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]')).find((button) => button.textContent === (whole ? "Whole message" : "This part"));
  if (control) await act(async () => control.click());
}
async function target(index: number) {
  const element = parts()[index];
  const article = element.closest<HTMLElement>('[role="article"]')!;
  await act(async () => article.focus());
  await act(async () => element.dispatchEvent(new MouseEvent("mousemove", { bubbles: true, clientY: index * 100 + 20 })));
  await act(async () => article.querySelector<HTMLButtonElement>('[aria-label="Message actions"]')!.focus());
  await scope(false);
}
async function click(label: string) { await act(async () => menuItem(label).click()); }

it("renders source-backed parts and sends the hovered source slice through both copy serializers", async () => {
  await render();
  expect(parts()).toHaveLength(5);
  expect(parts()[0].querySelector("h1")?.textContent).toBe("Opening");
  expect(parts()[0].querySelector("p")?.textContent).toBe("First paragraph.");
  await target(1);
  await click("Copy");
  expect(written).toEqual({ "text/plain": "Second paragraph.", [DOMIOS_TEXT_MIME]: "Second _paragraph_." });
  await click("Copy as Markdown");
  expect(written).toEqual({ "text/plain": "Second _paragraph_.", [DOMIOS_TEXT_MIME]: "Second _paragraph_." });
});

it("Quote excerpt, quote file content, and Ask content share the part target", async () => {
  await render(); await target(1); await click("Ask about this");
  expect(composerReplyStore.take("parts")).toEqual({ label: "Fermi", ref: "one", excerpt: "Second paragraph.", content: "Fermi:\n> Second _paragraph_." });
  expect(composerInsertionStore.take("parts")).toEqual([]);
});

it("Whole message switches the source and is remembered across other rows", async () => {
  await render(); await target(1); await scope(true); await click("Copy as Markdown");
  expect(written["text/plain"]).toBe(SOURCE);
  await click("Ask about this");
  const quoted = composerReplyStore.take("parts")?.content ?? "";
  expect(quoted).toContain("Opening");
  expect(quoted).toContain("Last paragraph.");
  const other = parts()[3].closest<HTMLElement>('[role="article"]')!;
  await act(async () => other.focus());
  await act(async () => other.querySelector<HTMLButtonElement>('[aria-label="Message actions"]')!.focus());
  const whole = Array.from(other.querySelectorAll('[role="menuitemradio"]')).find((button) => button.textContent === "Whole message");
  expect(whole?.getAttribute("aria-checked")).toBe("true");
  await scope(false);
});

it("Select picks a part and dragging extends across messages without adding its earlier siblings", async () => {
  await render(); await target(1); await click("Select");
  expect(toolbar()?.textContent).toContain("1 part");
  await act(async () => {
    parts()[1].dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0, shiftKey: true, clientY: 120 }));
    parts()[3].dispatchEvent(new MouseEvent("mousemove", { bubbles: true, clientY: 320 }));
    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
  });
  const copy = Array.from(toolbar()!.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent === "Copy")!;
  await act(async () => copy.click());
  expect(toolbar()?.textContent).toContain("3 parts");
  expect(written[DOMIOS_TEXT_MIME]).toBe("### Fermi\n\nSecond _paragraph_.\n\n### Fermi\n\nLast paragraph.\n\n### Fermi\n\nNext message.");
});

it("keyboard navigation selects ordered parts and Escape clears", async () => {
  await render(); await target(1); await click("Select");
  await act(async () => parts()[1].dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, code: "ArrowDown", shiftKey: true })));
  expect(toolbar()?.textContent).toContain("2 parts");
  await act(async () => document.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true })));
  expect(toolbar()).toBeNull();
});

it("retains original ranges through directive isolation and HTML normalization in the actual bubble", async () => {
  const html = "<div>First\r\n\r\nsecond</div>";
  const source = `Lead.\r\n@ds-plugin example\r\n\r\n${html}\r\n\r\nTail.`;
  await render([{ id: "normalized", type: "assistant", content: source }]);
  expect(parts()).toHaveLength(4);
  const spans = parts().map((element) => source.slice(Number(element.dataset.srcStart), Number(element.dataset.srcEnd)));
  expect(spans).toEqual(["Lead.", "@ds-plugin example", html, "Tail."]);
  await target(2); await click("Copy");
  expect(written[DOMIOS_TEXT_MIME]).toBe(html);
  await click("Ask about this");
  expect(composerReplyStore.take("parts")?.content ?? "").toContain("<div>First");
});

it("uses the keyboard-focused part as the action target", async () => {
  await render();
  await act(async () => parts()[2].focus());
  await act(async () => parts()[2].closest<HTMLElement>('[role="article"]')!.querySelector<HTMLButtonElement>('[aria-label="Message actions"]')!.focus());
  await scope(false); await click("Copy");
  expect(written[DOMIOS_TEXT_MIME]).toBe("Last paragraph.");
});
