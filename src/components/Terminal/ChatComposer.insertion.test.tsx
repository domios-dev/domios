import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { parseAttachmentMarkers } from "@codeterm/chat-core";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { ChatComposer } from "./ChatComposer";
import { capsuleSpans } from "./composerCapsules";
import { composerInsertionStore, insertIntoComposer } from "./composerInsertion";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const stored = vi.hoisted(() => ({ blobs: [] as { name: string; text: Promise<string> }[] }));
vi.mock("./voice/audio", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./voice/audio")>()),
  storeBlob: async (blob: Blob, name: string) => {
    stored.blobs.push({ name, text: blob.text() });
    return `/tmp/ct/${stored.blobs.length}-${name}`;
  },
}));

const muxState = (): ExtendedState => ({
  workspaces: [], tabs: {}, teams: [], active_workspace_id: null, focused_tab_id: null, notifications: [],
  tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [],
});
let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(async () => {
  await act(async () => root?.unmount()); root = null; host?.remove(); host = null;
  __setInvokeForTest(null); __setListenForTest(null); composerInsertionStore.clear(); stored.blobs.length = 0;
  try { window.localStorage.clear(); } catch { /* storage may be unavailable */ }
});

async function renderComposer(tabId: string) {
  const calls: { command: string; args: Record<string, unknown> }[] = [];
  __setInvokeForTest((async (command: string, args: Record<string, unknown> = {}) => {
    calls.push({ command, args });
    if (command === "delayed_send_list") return [];
    if (command === "chat_send_text") return { send_id: "s", tab_id: tabId, state: "written", error: null, ledger_id: "l", sequence: 1, retained_from: 1 };
    return {};
  }) as never);
  __setListenForTest((async () => () => {}) as never);
  const hooks = { addAttachments: vi.fn(), onWillSendMessage: vi.fn(), addOptimistic: vi.fn(() => "optimistic"), observeChatSendStatus: vi.fn() };
  host = document.createElement("div"); document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<MuxStateStoreContext.Provider value={createMuxStateStore(muxState)}>
    <ChatComposer tabId={tabId} tabProvider={null} currentPtyId={null} pendingPrompt={null} attachments={[]} addAttachments={hooks.addAttachments}
      removeAttachment={() => {}} clearAttachments={() => {}} handlePasteEvent={() => {}} onWillSendMessage={hooks.onWillSendMessage}
      addOptimistic={hooks.addOptimistic} updateOptimistic={() => {}} observeChatSendStatus={hooks.observeChatSendStatus} showComposerError={() => {}} />
  </MuxStateStoreContext.Provider>));
  const textarea = host.querySelector<HTMLTextAreaElement>('[data-testid="chat-composer"]')!;
  return { textarea, calls, hooks };
}

const labels = (textarea: HTMLTextAreaElement) => capsuleSpans(textarea.value).map((span) => span.label);
const sends = (calls: { command: string; args: Record<string, unknown> }[]) => calls.filter((call) => call.command === "chat_send_text");

it("lands every kind of context as an inline capsule, stores content to a file, and never sends", async () => {
  const { textarea, calls, hooks } = await renderComposer("insert-tab");
  await act(async () => insertIntoComposer("insert-tab", [
    { kind: "quote", detail: "domios-after.md:42", content: "From `/d/domios-after.md:42`:\n> quoted" },
    { kind: "message", label: "msg · Fermi 14:05", content: "### Fermi · 14:05\n\nDone." },
    { kind: "diff", label: "diff · ChangesPanel.tsx +26 −4", content: "```diff\n+a\n```" },
    { kind: "document", path: "/docs/report.md" },
  ]));
  expect(labels(textarea)).toEqual(["quote1 · domios-after.md:42", "msg · Fermi 14:05", "diff · ChangesPanel.tsx +26 −4", "report.md"]);
  expect(await Promise.all(stored.blobs.map((blob) => blob.text))).toEqual([
    "From `/d/domios-after.md:42`:\n> quoted", "### Fermi · 14:05\n\nDone.", "```diff\n+a\n```",
  ]);
  expect(stored.blobs.map((blob) => blob.name)).toEqual(["quote-domios-after.md-42.md", "message-msg-fermi-14-05.md", "diff-diff-changespanel.tsx-26-4.diff"]);
  expect(document.activeElement).toBe(textarea);
  expect(textarea.selectionStart).toBe(textarea.value.length);
  expect(sends(calls)).toEqual([]);
  expect(hooks.onWillSendMessage).not.toHaveBeenCalled();
  expect(hooks.addOptimistic).not.toHaveBeenCalled();
  expect(hooks.addAttachments).not.toHaveBeenCalled();

  await act(async () => { textarea.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", code: "Enter", bubbles: true, cancelable: true })); });
  await act(async () => { await Promise.resolve(); });
  const sent = String(sends(calls)[0]?.args.text ?? "");
  expect(parseAttachmentMarkers(sent).attachments).toEqual([
    { kind: "quote", path: "/tmp/ct/1-quote-domios-after.md-42.md", label: "quote1 · domios-after.md:42" },
    { kind: "message", path: "/tmp/ct/2-message-msg-fermi-14-05.md", label: "msg · Fermi 14:05" },
    { kind: "diff", path: "/tmp/ct/3-diff-diff-changespanel.tsx-26-4.diff", label: "diff · ChangesPanel.tsx +26 −4" },
    { kind: "document", path: "/docs/report.md", label: "report.md" },
  ]);
});

it("appends after the draft when the composer is not focused and numbers quotes per draft", async () => {
  const { textarea } = await renderComposer("append-tab");
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(textarea, "look at");
    textarea.setSelectionRange(0, 0);
    textarea.dispatchEvent(new Event("input", { bubbles: true }));
    textarea.blur();
  });
  await act(async () => insertIntoComposer("append-tab", { kind: "quote", detail: "a.md:1", content: "x" }));
  await act(async () => insertIntoComposer("append-tab", { kind: "quote", detail: "a.md:2", content: "y" }));
  expect(textarea.value.startsWith("look at ")).toBe(true);
  expect(labels(textarea)).toEqual(["quote1 · a.md:1", "quote2 · a.md:2"]);
});

it("ignores another tab's insertion and drains context queued before the composer mounted", async () => {
  insertIntoComposer("late-tab", { kind: "document", path: "/q/queued.md" });
  const { textarea } = await renderComposer("late-tab");
  expect(labels(textarea)).toEqual(["queued.md"]);
  await act(async () => insertIntoComposer("someone-else", { kind: "document", path: "/q/other.md" }));
  expect(labels(textarea)).toEqual(["queued.md"]);
  expect(composerInsertionStore.take("someone-else")).toEqual([{ kind: "document", path: "/q/other.md" }]);
});
