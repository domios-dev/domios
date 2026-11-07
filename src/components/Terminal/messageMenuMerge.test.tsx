import { act, useRef, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ChatBlockHostProvider, CompactToolRowsProvider, mergeMessageMenu, type ChatCardMenuItem } from "@codeterm/chat-ui";
import { SearchCard } from "../../../packages/chat-ui/src/toolcard/SearchCard";
import type { ToolEvent } from "@codeterm/chat-core";
import { ChatMessageRow } from "./ChatMessageRow";
import { renderChatCardMenu } from "./ChatCardMenu";
import { useChatMessageSelection } from "./useChatMessageSelection";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
vi.mock("../../lib/clientTabUiMode", () => ({ getCurrentTabUiMode: () => "chat", setClientTabUiMode: vi.fn() }));

const item = (label: string, kind?: ChatCardMenuItem["kind"]): ChatCardMenuItem => ({ label, kind, onSelect: () => {} });

describe("the shared message menu model", () => {
  it("merges a card into its message once, in one order, with the card's Copy and Ask", () => {
    const message = [item("Copy", "copy"), item("Ask about this", "ask"), item("Select", "select"), item("Delete", "delete")];
    const cardAsk = item("Ask about this", "ask");
    const card = [item("Open full screen", "open"), item("Copy command"), item("Copy output", "copy"), cardAsk, item("Details", "details")];
    const merged = mergeMessageMenu(message, card);
    expect(merged.map((entry) => entry.label)).toEqual(["Open full screen", "Copy", "Copy command", "Ask about this", "Details", "Select", "Delete"]);
    expect(merged.find((entry) => entry.kind === "ask")).toBe(cardAsk);
    expect(mergeMessageMenu(message, []).map((entry) => entry.label)).toEqual(["Copy", "Ask about this", "Select", "Delete"]);
  });
});

const event: ToolEvent = {
  id: "search", phase: "result", status: "success", kind: "search", summary: "search 10 matches in 2 files", result: "a\nb",
  message: { id: "search", type: "tool_call", content: "", toolName: "Grep", toolInput: { pattern: "/model" } },
};
const msg = { id: "search", type: "tool_call" as const, content: "", toolName: "Grep", toolInput: { pattern: "/model" } };

function Harness({ children }: { children: ReactNode }) {
  const surfaceRef = useRef<HTMLDivElement>(null);
  const selection = useChatMessageSelection({ tabId: "menus", agent: "Fermi", surfaceRef, rows: [{ msg, item: { kind: "tool", event } }] as never });
  return <CompactToolRowsProvider surfaceRef={surfaceRef}><div ref={surfaceRef} role="log">
    <ChatMessageRow msg={msg} ariaLabel="search" selected={false} joinNext={false} selecting={false} cursor handlers={selection.rowHandlers}>{children}</ChatMessageRow>
  </div></CompactToolRowsProvider>;
}

let root: Root | undefined;
let host: HTMLDivElement;
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  vi.restoreAllMocks();
});

async function mount(phone: boolean) {
  const matchMedia = window.matchMedia.bind(window);
  vi.spyOn(window, "matchMedia").mockImplementation((query) => {
    const media = matchMedia(query);
    if (query === "(pointer: coarse)") Object.defineProperty(media, "matches", { value: phone });
    return media;
  });
  const bounds = HTMLElement.prototype.getBoundingClientRect;
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    return { ...bounds.call(this), width: phone ? 390 : 900 };
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(<ChatBlockHostProvider value={{ renderMenu: renderChatCardMenu, askAbout: () => {} }}>
    <Harness><SearchCard event={event} expanded={false} onToggle={() => {}} /></Harness>
  </ChatBlockHostProvider>));
}

const labels = () => [...document.querySelectorAll('[role^="menuitem"]')].map((node) => node.getAttribute("aria-label") ?? node.textContent?.trim() ?? "");
const cardKebab = () => document.querySelector<HTMLButtonElement>('button[aria-label="Actions for Search"]');
const dot = () => document.querySelector<HTMLButtonElement>('button[aria-label="Message actions"]');

describe("one menu per tool row", () => {
  it("desktop: the card draws no ⋮ and the side-dot menu carries the merged items", async () => {
    await mount(false);
    expect(cardKebab()).toBeNull();
    const row = host.querySelector<HTMLElement>('[data-testid="chat-message"]')!;
    await act(async () => row.focus());
    await act(async () => dot()!.click());
    const items = labels();
    expect(items).toEqual(["Open full screen", "Copy", "Ask about this", "Details", "Select"]);
    expect(new Set(items).size).toBe(items.length);
  });

  it("touch: the card's ⋮ is the only menu and carries the same merged items", async () => {
    await mount(true);
    expect(dot()).toBeNull();
    await act(async () => cardKebab()!.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, cancelable: true })));
    const items = labels();
    expect(items).toEqual(["Open full screen", "Copy", "Ask about this", "Details", "Select"]);
    expect(new Set(items).size).toBe(items.length);
  });
});
