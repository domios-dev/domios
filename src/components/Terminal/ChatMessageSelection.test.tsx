import { act, useRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { DOMIOS_TEXT_MIME, formatAttachmentMarker, materializeToolEvent, type NormalizedChatMessage } from "@codeterm/chat-core";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { ChatComposer } from "./ChatComposer";
import { RichMarkdown } from "@codeterm/chat-ui";
import { ChatMessageRow } from "./ChatMessageRow";
import { capsuleSpans } from "./composerCapsules";
import { composerInsertionStore } from "./composerInsertion";
import { composerReplyStore } from "./composerReply";
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
    <div ref={surfaceRef} role="log" onKeyDown={selection.onKeyDown}>
      {rows.map(({ msg }, index) => (
        <ChatMessageRow key={msg.id} msg={msg} ariaLabel={msg.id}
          selected={selection.selected.has(msg.id)}
          joinNext={selection.selected.has(rows[index + 1]?.msg.id ?? "")}
          selecting={selection.active}
          cursor={selection.cursor ? selection.cursor === msg.id : index === rows.length - 1}
          handlers={selection.rowHandlers}>
          {msg.type === "assistant" ? <RichMarkdown text={msg.content} /> : <p>{msg.content}</p>}
        </ChatMessageRow>
      ))}
      <MessageSelectionBar count={selection.count} onCopy={selection.copySelection} onQuote={selection.quoteSelection} onAsk={selection.askSelection} onClear={selection.clear} />
    </div>
  );
}

let root: Root | null = null;
let host: HTMLDivElement | null = null;
let clipboard: Record<string, string>[] = [];
let calls: { command: string; args: Record<string, unknown> }[] = [];
let execCommandDescriptor: PropertyDescriptor | undefined;

function resetSharedState() {
  __setInvokeForTest(null); __setListenForTest(null);
  composerInsertionStore.clear(); composerReplyStore.clear(); stored.blobs.length = 0;
  document.getSelection()?.removeAllRanges();
  window.localStorage.clear(); window.sessionStorage.clear();
}

beforeEach(() => {
  vi.useRealTimers();
  resetSharedState();
  clipboard = [];
  calls = [];
  execCommandDescriptor = Object.getOwnPropertyDescriptor(document, "execCommand");
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
  resetSharedState();
  vi.restoreAllMocks();
  if (execCommandDescriptor) Object.defineProperty(document, "execCommand", execCommandDescriptor);
  else Reflect.deleteProperty(document, "execCommand");
});

async function render(tabId: string, rows = ROWS) {
  __setInvokeForTest((async (command: string, args: Record<string, unknown> = {}) => {
    calls.push({ command, args });
    return command === "delayed_send_list" ? [] : {};
  }) as never);
  __setListenForTest((async () => () => {}) as never);
  host = document.createElement("div"); document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<MuxStateStoreContext.Provider value={createMuxStateStore(muxState)}>
    <Harness tabId={tabId} rows={rows} />
    <ChatComposer tabId={tabId} tabProvider={null} currentPtyId={null} pendingPrompt={null} attachments={[]} addAttachments={() => {}}
      removeAttachment={() => {}} clearAttachments={() => {}} handlePasteEvent={() => {}} onWillSendMessage={() => {}}
      addOptimistic={() => "optimistic"} updateOptimistic={() => {}} observeChatSendStatus={() => {}} showComposerError={() => {}} />
  </MuxStateStoreContext.Provider>));
  // The composer's mount focus must finish before a row takes focus.
  await vi.waitFor(() => expect(document.activeElement).toBe(composer()));
}

const row = (id: string) => host!.querySelector<HTMLElement>(`[data-message-id="${id}"]`)!;
const dot = (id: string) => row(id).querySelector<HTMLButtonElement>('button[aria-label="Message actions"]');
const menu = (id: string) => row(id).querySelector<HTMLElement>('[role="menu"]');
const item = (id: string, label: string) => menu(id)?.querySelector<HTMLButtonElement>(`[role="menuitem"][aria-label="${label}"]`) ?? null;
const barButton = (text: string) => Array.from(host!.querySelectorAll<HTMLButtonElement>('[role="toolbar"][aria-label="Selected messages"] button')).find((button) => button.textContent === text)!;
const bar = () => host!.querySelector('[role="toolbar"][aria-label="Selected messages"]');
const composer = () => host!.querySelector<HTMLTextAreaElement>('[data-testid="chat-composer"]')!;
const capsules = () => capsuleSpans(composer().value).map((span) => span.label);
const replyBlock = () => host!.querySelector<HTMLElement>('[data-testid="reply-quote"]');
const sends = () => calls.filter(({ command }) => /send|write_pty|submit/i.test(command) && command !== "delayed_send_list");
const settle = () => act(async () => { for (let i = 0; i < 5; i += 1) await Promise.resolve(); });

async function hover(id: string) {
  await act(async () => row(id).focus());
  return vi.waitFor(() => {
    const trigger = dot(id);
    expect(trigger).not.toBeNull();
    return trigger!;
  });
}
async function openMenu(id: string) {
  const trigger = await hover(id);
  await act(async () => trigger.focus());
  await vi.waitFor(() => expect(menu(id)).not.toBeNull());
}
async function keyOn(target: Element, code: string, shiftKey = false, key = code) {
  await act(async () => { target.dispatchEvent(new KeyboardEvent("keydown", { code, key, shiftKey, bubbles: true, cancelable: true })); });
}
async function key(code: string, shiftKey = false) { await keyOn(document.activeElement ?? document.body, code, shiftKey); }
function mouse(target: Element, type: string, init: MouseEventInit = {}) {
  return target.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, button: 0, ...init }));
}
async function shiftClick(id: string) {
  await act(async () => {
    const text = row(id).querySelector("p")!;
    mouse(text, "mousedown", { shiftKey: true });
    mouse(document.body, "mouseup", { shiftKey: true });
    mouse(text, "click", { shiftKey: true });
  });
}
async function typeInComposer(text: string) {
  await act(async () => {
    const el = composer();
    el.focus();
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")?.set?.call(el, text);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

it("shows a quiet dot only for the row under the pointer or focus, and none while selecting", async () => {
  await render("dot-tab");
  expect(dot("a1")).toBeNull();
  await hover("a1");
  expect(dot("a1")?.getAttribute("aria-expanded")).toBe("false");
  expect(dot("a1")?.closest("[data-message-gutter]")).not.toBeNull();
  expect(dot("u1")).toBeNull();
  expect(menu("a1")).toBeNull();
  await shiftClick("t1");
  expect(dot("a1")).toBeNull();
});

it("places the action in the right gutter for every message kind", async () => {
  await render("right-dot-tab");
  for (const id of ["u1", "a1", "t1"]) {
    await hover(id);
    expect(dot(id)?.closest("[data-message-gutter]")?.getAttribute("data-side")).toBe("end");
  }
});

it("shows beads only in block mode and connects a selected range", async () => {
  await render("bead-tab");
  expect(host!.querySelector(".ct-msg-bead")).toBeNull();
  await shiftClick("u1");
  expect(host!.querySelectorAll(".ct-msg-bead")).toHaveLength(4);
  expect(row("u1").querySelector(".ct-msg-bead")?.getAttribute("data-selected")).toBe("true");
  expect(row("a1").querySelector(".ct-msg-bead")?.hasAttribute("data-selected")).toBe(false);
  await shiftClick("a2");
  expect(host!.querySelectorAll(".ct-msg-thread")).toHaveLength(3);
  expect(host!.querySelectorAll(".ct-msg-bead[data-selected]")).toHaveLength(4);
  await act(async () => { mouse(row("t1").querySelector('[data-side="end"]')!, "mousedown"); mouse(document.body, "mouseup"); });
  expect(row("t1").querySelector(".ct-msg-bead")?.hasAttribute("data-selected")).toBe(false);
  expect(host!.querySelectorAll(".ct-msg-thread")).toHaveLength(1);
});

it("reaches the dot by Tab from its row and opens the menu with Enter, Esc closes it", async () => {
  await render("keys-menu-tab");
  const trigger = await hover("a2");
  const focusable = row("a2").querySelector<HTMLElement>('button, a[href], input, textarea, [tabindex="0"]');
  expect(focusable).toBe(trigger);
  await keyOn(trigger, "Enter");
  await vi.waitFor(() => expect(menu("a2")).not.toBeNull());
  expect(document.activeElement).toBe(item("a2", "Copy"));
  await key("ArrowDown");
  expect(document.activeElement).toBe(item("a2", "Ask about this"));
  await key("Escape");
  expect(menu("a2")).toBeNull();
  expect(document.activeElement).toBe(dot("a2"));
  expect(bar()).toBeNull();
});

it("copies a message as it really is from the menu, once", async () => {
  await render("copy-tab");
  await openMenu("u1");
  expect(item("u1", "Copy as Markdown")).not.toBeNull();
  await act(async () => item("u1", "Copy")!.click());
  expect(document.execCommand).toHaveBeenCalledTimes(1);
  expect(clipboard).toEqual([{ "text/plain": "fix image1 please", [DOMIOS_TEXT_MIME]: `fix ${IMAGE} please` }]);
  expect(item("u1", "Copied")).not.toBeNull();
});

it("offers Copy as Markdown only where the rendered text differs, and copies the exact source", async () => {
  await render("md-tab");
  await openMenu("a2");
  expect(item("a2", "Copy as Markdown")).toBeNull();
  await openMenu("a1");
  await act(async () => item("a1", "Copy as Markdown")!.click());
  expect(clipboard).toEqual([{ "text/plain": "Done **now**.", [DOMIOS_TEXT_MIME]: "Done **now**." }]);
});

it("copies a tool row as its command and output", async () => {
  await render("tool-tab");
  await openMenu("t1");
  await act(async () => item("t1", "Copy")!.click());
  expect(clipboard[0]["text/plain"]).toBe("$ ls\na.ts");
});

it("the menu offers Ask about this and no separate Quote item", async () => {
  await render("ask-tab");
  await openMenu("a1");
  expect(item("a1", "Quote into composer")).toBeFalsy();
  expect(item("a1", "Ask about this")).toBeTruthy();
});

it("Select enters block selection with that message", async () => {
  await render("select-item-tab");
  await openMenu("a1");
  await act(async () => item("a1", "Select")!.click());
  expect(bar()?.textContent).toContain("1 message");
  expect(row("a1").dataset.selected).toBe("true");
  expect(document.activeElement).toBe(row("a1"));
});

it("Quote docks one reply block above the composer input, replaces it, and never inserts a capsule", async () => {
  await render("quote-tab");
  await openMenu("a1");
  await act(async () => item("a1", "Ask about this")!.click());
  await settle();
  expect(menu("a1")).toBeNull();
  expect(replyBlock()?.getAttribute("aria-label")).toBe("Replying to Fermi");
  expect(replyBlock()?.textContent).toContain("Done now.");
  expect(composer().value).toBe("");
  expect(await stored.blobs[0].text).toBe("Fermi:\n> Done **now**.");
  await openMenu("a2");
  await act(async () => item("a2", "Ask about this")!.click());
  await settle();
  expect(host!.querySelectorAll('[data-testid="reply-quote"]')).toHaveLength(1);
  expect(replyBlock()?.textContent).toContain("plain reply");
  await act(async () => replyBlock()!.querySelector<HTMLButtonElement>('button[aria-label="Remove quote"]')!.click());
  expect(replyBlock()).toBeNull();
  expect(sends()).toEqual([]);
});

it("Esc in an empty composer removes the reply block", async () => {
  await render("quote-esc-tab");
  await openMenu("a1");
  await act(async () => item("a1", "Ask about this")!.click());
  await settle();
  await act(async () => composer().focus());
  await keyOn(composer(), "Escape");
  expect(replyBlock()).toBeNull();
});

it("sends the reply once as its quote attachment ahead of the text, then clears it", async () => {
  await render("quote-send-tab");
  await openMenu("a1");
  await act(async () => item("a1", "Ask about this")!.click());
  await settle();
  await typeInComposer("why?");
  await keyOn(composer(), "Enter", false, "Enter");
  await settle();
  const sent = sends();
  expect(sent).toHaveLength(1);
  const text = String(sent[0].args.text);
  expect(text).toBe(`${formatAttachmentMarker("/tmp/ct/1-quote-fermi.md", "quote", "Fermi", { ref: "a1", excerpt: "Done now." })}\nwhy?`);
  expect(text.match(/kind="quote"/g)).toHaveLength(1);
  expect(replyBlock()).toBeNull();
});

it("selects with Shift+click, copies in order with role headings, and asks about the selection as one quote", async () => {
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
  await settle();
  expect(replyBlock()?.getAttribute("aria-label")).toBe("Replying to 2 messages");
  expect(capsules()).toEqual([]);
  expect(bar()).toBeNull();
  expect(sends()).toEqual([]);
});

it("Quote on a block selection docks one reply for all of it", async () => {
  await render("quote-many-tab");
  await shiftClick("a1");
  await shiftClick("a2");
  await act(async () => barButton("Ask about these").click());
  await settle();
  expect(replyBlock()?.getAttribute("aria-label")).toBe("Replying to 3 messages");
  expect(capsules()).toEqual([]);
  expect(bar()).toBeNull();
});

it("pressing and dragging in the gutter selects whole rows, not text", async () => {
  await render("drag-tab");
  const gutter = row("u1").querySelector("[data-message-gutter]")!;
  let allowed = true;
  await act(async () => { allowed = mouse(gutter, "mousedown", { clientY: 10 }); });
  expect(allowed).toBe(false);
  expect(bar()).toBeNull();
  await act(async () => {
    mouse(row("a1").querySelector("p")!, "mousemove", { clientY: 60 });
    mouse(row("t1").querySelector("p")!, "mousemove", { clientY: 110 });
  });
  expect(bar()?.textContent).toContain("3 messages");
  await act(async () => { mouse(row("a1").querySelector("p")!, "mousemove", { clientY: 60 }); });
  expect(bar()?.textContent).toContain("2 messages");
  await act(async () => { mouse(document.body, "mouseup"); });
  expect(["u1", "a1", "t1", "a2"].map((id) => row(id).dataset.selected === "true")).toEqual([true, true, false, false]);
  expect(document.getSelection()?.toString() ?? "").toBe("");
});

it("in block mode a drag over message text picks rows, and Esc leaves block mode", async () => {
  await render("block-drag-tab");
  await shiftClick("u1");
  let allowed = true;
  await act(async () => { allowed = mouse(row("t1").querySelector("p")!, "mousedown", { clientY: 100 }); });
  expect(allowed).toBe(false);
  await act(async () => {
    mouse(row("a2").querySelector("p")!, "mousemove", { clientY: 150 });
    mouse(document.body, "mouseup");
  });
  expect(bar()?.textContent).toContain("3 messages");
  await act(async () => { document.body.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", key: "Escape", bubbles: true, cancelable: true })); });
  expect(bar()).toBeNull();
  expect(host!.querySelector("[data-selected]")).toBeNull();
});

it("outside block mode a press on message text stays a text selection", async () => {
  await render("text-tab");
  let allowed = false;
  await act(async () => { allowed = mouse(row("a1").querySelector("p")!, "mousedown", { clientY: 60 }); });
  expect(allowed).toBe(true);
  await act(async () => {
    mouse(row("a2").querySelector("p")!, "mousemove", { clientY: 160 });
    mouse(document.body, "mouseup");
  });
  expect(bar()).toBeNull();
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
});

it("materializes only the targeted part in the quote file", async () => {
  const source = "First paragraph.\n\nSecond **paragraph**.";
  await render("part-files", [asRow(message("parts", "assistant", source))]);
  await openMenu("parts");
  await act(async () => item("parts", "Ask about this")!.click());
  expect(replyBlock()?.textContent).toContain("First paragraph.");
  expect(await stored.blobs[0].text).toBe("Fermi:\n> First paragraph.");
  expect(sends()).toEqual([]);
});
