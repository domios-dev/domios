import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, expect, it, vi } from "vitest";
import { configureDaemonFixture } from "../../test/daemonTransport.testdata";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { setAgentDescriptors } from "../../lib/agentDescriptorCache";
import type { ProviderDescriptor } from "../../lib/types";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { ComposerIssuesProvider } from "./ComposerIssuesContext";
import { ChatComposer } from "./ChatComposer";

beforeEach(() => configureDaemonFixture());
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const readable = { filePaths: true, inlineText: true, imagePaths: true };
const inline = { filePaths: false, inlineText: true, imagePaths: false };

it.each([
  ["file", "/opaque/note.txt", inline, false],
  ["image", "/opaque/photo.png", inline, false],
  ["file", "/opaque/note.txt", readable, true],
  ["image", "/opaque/photo.png", readable, true],
  ["unknown", "/opaque/note.txt", undefined, false],
] as const)("composer checks %s path capability before clearing: %s", async (_kind, path, capabilities, allowed) => {
  const descriptor = { id: "opaque", attachments: capabilities } as ProviderDescriptor;
  setAgentDescriptors([descriptor]);
  const sent: string[] = [];
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    if (command === "chat_send_text") {
      sent.push(String(args?.text));
      return { send_id: "sent", tab_id: "capability", state: "delivered", ledger_id: "ledger", sequence: 1, retained_from: 0 } as T;
    }
    if (["delayed_send_list", "cron_list"].includes(command)) return [] as T;
    return { items: [], receiverTargets: {} } as T;
  });
  __setListenForTest(async () => () => {});
  const state = { workspaces: [], tabs: {}, teams: [], notifications: [], tabStatuses: {}, toasts: [],
    activeTabIds: {}, generation: 1, pendingCloses: [], active_workspace_id: null, focused_tab_id: null } as ExtendedState;
  const store = createMuxStateStore(() => state);
  const clear = vi.fn();
  const optimistic = vi.fn(() => "sent");
  const error = vi.fn();
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<MuxStateStoreContext.Provider value={store}><ComposerIssuesProvider>
      <ChatComposer tabId="capability" tabProvider="opaque" currentPtyId={null} chatBackend pendingPrompt={null}
        attachments={[path]} addAttachments={() => {}} removeAttachment={() => {}} clearAttachments={clear}
        handlePasteEvent={() => {}} onWillSendMessage={() => {}} addOptimistic={optimistic}
        updateOptimistic={() => {}} observeChatSendStatus={() => {}} showComposerError={error} />
    </ComposerIssuesProvider></MuxStateStoreContext.Provider>));
    const editor = host.querySelector<HTMLTextAreaElement>("textarea")!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(editor, "inspect opaque");
      editor.dispatchEvent(new Event("input", { bubbles: true }));
    });
    if (!allowed) {
      const paste = Object.assign(new Event("paste", { bubbles: true, cancelable: true }), {
        clipboardData: { files: [], items: [], getData: () => "opaque text\n".repeat(500) },
      });
      await act(async () => editor.dispatchEvent(paste));
      expect(paste.defaultPrevented).toBe(false);
    }
    await act(async () => editor.dispatchEvent(new KeyboardEvent("keydown", { code: "Enter", key: "Enter", bubbles: true })));
    expect(clear).toHaveBeenCalledTimes(allowed ? 1 : 0);
    expect(optimistic).toHaveBeenCalledTimes(allowed ? 1 : 0);
    expect(sent).toHaveLength(allowed ? 1 : 0);
    if (allowed) expect(sent[0]).toContain(path);
    else {
      expect(editor.value).toBe("inspect opaque");
      expect(editor.getAttribute("aria-invalid")).toBe("true");
      expect(error).toHaveBeenCalledOnce();
    }
  } finally {
    await act(async () => root.unmount()); host.remove(); setAgentDescriptors([]);
  }
});
