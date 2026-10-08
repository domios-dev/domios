import { act, useRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { DOMIOS_TEXT_MIME, formatAttachmentMarker, materializeToolEvent, type NormalizedChatMessage } from "@codeterm/chat-core";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { ChatComposer } from "./ChatComposer";
import { ChatMessageRow } from "./ChatMessageRow";
import { capsuleSpans } from "./composerCapsules";
import { composerInsertionStore } from "./composerInsertion";
import { MessageSelectionBar } from "./MessageSelectionBar";
import { useChatMessageSelection, type SelectableRow } from "./useChatMessageSelection";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const stored = vi.hoisted(() => ({ blobs: [] as { name: string; text: Promise<string> }[] }));
vi.mock("./voice/audio", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./voice/audio")>()),
  storeBlob: async (blob: Blob, name: string) => {
    stored.blobs.push({ name, text: blob.text() });
    return `/tmp/ct/${stored.blobs.length}-${name}`;
  },
}));

const IMAGE = formatAttachmentMarker("/tmp/shot.png", "image", "image1");
const message = (id: string, type: NormalizedChatMessage["type"], content: string, extra: Partial<NormalizedChatMessage> = {}): NormalizedChatMessage => ({ id, type, content, ...extra });
const asRow = (msg: NormalizedChatMessage): SelectableRow => {
  const event = materializeToolEvent(msg);
  return { msg, item: event ? { kind: "tool", event } : { kind: "message", message: msg } };
};
const ROWS: SelectableRow[] = [
  asRow(message("u1", "user", `fix ${IMAGE} please`)),
  asRow(message("a1", "assistant", "Done **now**.")),
  asRow(message("t1", "tool_call", "", { toolName: "Bash", toolKind: "command", toolInput: { command: "ls" }, toolResult: "a.ts\n" })),
  asRow(message("a2", "assistant", "plain reply")),
];

const muxState = (): ExtendedState => ({
  workspaces: [], tabs: {}, teams: [], active_workspace_id: null, focused_tab_id: null, notifications: [],
  tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [],
});

function Harness({ tabId, rows }: { tabId: string; rows: SelectableRow[] }) {
  const surfaceRef = useRef<HTMLDivElement>(null);
  const selection = useChatMessageSelection({ tabId, rows, agent: "Fermi", surfaceRef });
  return (
    <div ref={surfaceRef} onKeyDown={selection.onKeyDown}>
      {rows.map(({ msg }, index) => (
        <ChatMessageRow key={msg.id} msg={msg} ariaLabel={msg.id}
          selected={selection.selected.has(msg.id)}
          joinNext={selection.selected.has(rows[index + 1]?.msg.id ?? "")}
          selecting={selection.active}
          cursor={selection.cursor ? selection.cursor === msg.id : index === rows.length - 1}
          handlers={selection.rowHandlers}>
          <p>{msg.content}</p>
        </ChatMessageRow>
      ))}
      <MessageSelectionBar count={selection.count} onCopy={selection.copySelection} onQuote={selection.quoteSelection} onAsk={selection.askSelection} onClear={selection.clear} />
    </div>
  );
}

let root: Root | null = null;
let host: HTMLDivElement | null = null;
let clipboard: Record<string, string>[] = [];
let calls: string[] = [];

beforeEach(() => {
  clipboard = [];
  calls = [];
  document.execCommand = vi.fn((command: string) => {
    if (command !== "copy") return false;
    const written: Record<string, string> = {};
    const event = Object.assign(new Event("copy", { bubbles: true, cancelable: true }), {
      clipboardData: { setData: (format: string, value: string) => { written[format] = value; } },
    });
    document.dispatchEvent(event);
    clipboard.push(written);
    return true;
  }) as typeof document.execCommand;
});

afterEach(async () => {
  await act(async () => root?.unmount()); root = null; host?.remove(); host = null;
  __setInvokeForTest(null); __setListenForTest(null); composerInsertionStore.clear(); stored.blobs.length = 0;
  try { window.localStorage.clear(); } catch { /* storage may be unavailable */ }
});

async function render(tabId: string, rows = ROWS) {
  __setInvokeForTest((async (command: string) => { calls.push(command); return command === "delayed_send_list" ? [] : {}; }) as never);
  __setListenForTest((async () => () => {}) as never);
  host = document.createElement("div"); document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<MuxStateStoreContext.Provider value={createMuxStateStore(muxState)}>
    <Harness tabId={tabId} rows={rows} />
    <ChatComposer tabId={tabId} tabProvider={null} currentPtyId={null} pendingPrompt={null} attachments={[]} addAttachments={() => {}}
      removeAttachment={() => {}} clearAttachments={() => {}} handlePasteEvent={() => {}} onWillSendMessage={() => {}}
      addOptimistic={() => "optimistic"} updateOptimistic={() => {}} observeChatSendStatus={() => {}} showComposerError={() => {}} />
  </MuxStateStoreContext.Provider>));
}

const row = (id: string) => host!.querySelector<HTMLElement>(`[data-message-id="${id}"]`)!;
const button = (scope: ParentNode, label: string) => scope.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`);
const barButton = (text: string) => Array.from(host!.querySelectorAll<HTMLButtonElement>('[role="toolbar"][aria-label="Selected messages"] button')).find((item) => item.textContent === text)!;
const bar = () => host!.querySelector('[role="toolbar"][aria-label="Selected messages"]');
const composer = () => host!.querySelector<HTMLTextAreaElement>('[data-testid="chat-composer"]')!;
const capsules = () => capsuleSpans(composer().value).map((span) => span.label);
const sends = () => calls.filter((command) => /send|write_pty|submit/i.test(command) && command !== "delayed_send_list");
async function hover(id: string) { await act(async () => row(id).focus()); }
async function key(code: string, shiftKey = false) {
  await act(async () => { (document.activeElement ?? document.body).dispatchEvent(new KeyboardEvent("keydown", { code, key: code, shiftKey, bubbles: true, cancelable: true })); });
}
async function shiftClick(id: string) {
  await act(async () => {
    row(id).querySelector("p")!.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true, shiftKey: true, button: 0 }));
    row(id).querySelector("p")!.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, shiftKey: true, button: 0 }));
  });
}

it("copies a message as it really is: labels in plain text, attachment tags in the portable flavour, once", async () => {
  await render("copy-tab");
  await hover("u1");
  expect(button(row("u1"), "Copy as Markdown")).toBeTruthy();
  await act(async () => button(row("u1"), "Copy message")!.click());
  expect(document.execCommand).toHaveBeenCalledTimes(1);
  expect(clipboard).toEqual([{ "text/plain": "fix image1 please", [DOMIOS_TEXT_MIME]: `fix ${IMAGE} please` }]);
  expect(button(row("u1"), "Copied")).toBeTruthy();
});

it("offers Copy as Markdown only where the rendered text differs, and copies the exact source", async () => {
  await render("md-tab");
  await hover("a2");
  expect(button(row("a2"), "Copy as Markdown")).toBeNull();
  await hover("a1");
  await act(async () => button(row("a1"), "Copy as Markdown")!.click());
  expect(clipboard).toEqual([{ "text/plain": "Done **now**.", [DOMIOS_TEXT_MIME]: "Done **now**." }]);
  await act(async () => button(row("a1"), "Copy message")!.click());
  expect(clipboard[1]["text/plain"]).toBe("Done now.");
});

it("copies a tool row as its command and output", async () => {
  await render("tool-tab");
  await hover("t1");
  await act(async () => button(row("t1"), "Copy message")!.click());
  expect(clipboard[0]["text/plain"]).toBe("$ ls\na.ts");
});

it("quotes one message into the composer as a capsule without sending", async () => {
  await render("quote-tab");
  await hover("a1");
  await act(async () => button(row("a1"), "Quote into composer")!.click());
  expect(capsules()).toEqual(["quote1 · Fermi"]);
  expect(await stored.blobs[0].text).toBe("Fermi:\n> Done **now**.");
  expect(sends()).toEqual([]);
});

it("selects with Shift+click, copies in order with role headings, and asks about the selection as one capsule", async () => {
  await render("select-tab");
  await shiftClick("a1");
  await shiftClick("t1");
  expect(bar()?.textContent).toContain("2 messages");
  expect(row("a1").dataset.joinNext).toBe("true");
  await act(async () => barButton("Copy").click());
  expect(clipboard).toEqual([{
    "text/plain": "Fermi\nDone now.\n\nTool · Bash\n$ ls\na.ts",
    [DOMIOS_TEXT_MIME]: "### Fermi\n\nDone **now**.\n\n### Tool · Bash\n\n$ ls\na.ts",
  }]);
  await act(async () => barButton("Ask about these").click());
  expect(capsules()).toEqual(["2 messages"]);
  expect(await stored.blobs[0].text).toBe("### Fermi\n\nDone **now**.\n\n### Tool · Bash\n\n$ ls\na.ts");
  expect(bar()).toBeNull();
  expect(sends()).toEqual([]);
});

it("clicks toggle rows while selecting and Quote takes the whole selection", async () => {
  await render("toggle-tab");
  await shiftClick("u1");
  await act(async () => row("a2").querySelector("p")!.click());
  await act(async () => row("u1").querySelector("p")!.click());
  expect(bar()?.textContent).toContain("1 message");
  await act(async () => barButton("Quote").click());
  expect(capsules()).toEqual(["quote1 · Fermi"]);
});

it("moves with arrows, toggles with Space, extends with Shift+arrows and clears with Escape", async () => {
  await render("keys-tab");
  expect(row("a2").tabIndex).toBe(0);
  await act(async () => row("a2").focus());
  await key("ArrowUp");
  expect(document.activeElement).toBe(row("t1"));
  await key("Space");
  expect(bar()?.textContent).toContain("1 message");
  await key("ArrowUp", true);
  await key("ArrowUp", true);
  expect(bar()?.textContent).toContain("3 messages");
  expect(document.activeElement).toBe(row("u1"));
  await key("ArrowDown", true);
  expect(bar()?.textContent).toContain("2 messages");
  await key("Escape");
  expect(bar()).toBeNull();
});

it("leaves a native text selection alone", async () => {
  await render("native-tab");
  await act(async () => {
    const range = document.createRange();
    range.selectNodeContents(row("a1").querySelector("p")!);
    document.getSelection()?.removeAllRanges();
    document.getSelection()?.addRange(range);
  });
  await shiftClick("a1");
  expect(bar()).toBeNull();
  document.getSelection()?.removeAllRanges();
});
