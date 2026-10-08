import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { ChatComposer } from "./ChatComposer";
import { composerInsertionStore, insertIntoComposer } from "./composerInsertion";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const muxState = (): ExtendedState => ({
  workspaces: [], tabs: {}, teams: [], active_workspace_id: null, focused_tab_id: null, notifications: [],
  tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [],
});
let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(async () => {
  await act(async () => root?.unmount()); root = null; host?.remove(); host = null;
  __setInvokeForTest(null); __setListenForTest(null); composerInsertionStore.clear();
  try { window.localStorage.clear(); } catch { /* storage may be unavailable */ }
});

async function renderComposer(tabId: string) {
  const commands: string[] = [];
  __setInvokeForTest((async (command: string) => { commands.push(command); return command === "delayed_send_list" ? [] : {}; }) as never);
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
  return { textarea, commands, hooks };
}

it("appends panel context to the draft, adds file chips and focuses the composer without sending", async () => {
  const { textarea, commands, hooks } = await renderComposer("insert-tab");
  await act(async () => insertIntoComposer("insert-tab", { attachments: ["/docs/a.md"], text: "From `/docs/a.md:3`:\n> quoted" }));
  expect(textarea.value).toBe("From `/docs/a.md:3`:\n> quoted\n");
  expect(hooks.addAttachments).toHaveBeenCalledWith(["/docs/a.md"]);
  expect(document.activeElement).toBe(textarea);
  expect(textarea.selectionStart).toBe(textarea.value.length);
  await act(async () => insertIntoComposer("insert-tab", { text: "second" }));
  expect(textarea.value).toBe("From `/docs/a.md:3`:\n> quoted\n\nsecond\n");
  expect(hooks.onWillSendMessage).not.toHaveBeenCalled();
  expect(hooks.addOptimistic).not.toHaveBeenCalled();
  expect(hooks.observeChatSendStatus).not.toHaveBeenCalled();
  expect(commands.filter((command) => command !== "delayed_send_list" && /send|write/i.test(command))).toEqual([]);
});

it("ignores another tab's insertion and drains context queued before the composer mounted", async () => {
  insertIntoComposer("late-tab", { text: "queued" });
  const { textarea } = await renderComposer("late-tab");
  expect(textarea.value).toBe("queued\n");
  await act(async () => insertIntoComposer("someone-else", { text: "not mine" }));
  expect(textarea.value).toBe("queued\n");
  expect(composerInsertionStore.take("someone-else")).toEqual([{ text: "not mine" }]);
});
