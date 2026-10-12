import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { parseAttachmentMarkers } from "@codeterm/chat-core";
import { capsuleSpans } from "./composerCapsules";
import { ChatComposer } from "./ChatComposer";
import { useAttachments } from "./useAttachments";

const fixture = vi.hoisted(() => ({ web: true, stored: [] as Blob[] }));
vi.mock("../../lib/webApi", async (original) => ({
  ...await original<typeof import("../../lib/webApi")>(),
  get IS_WEB() { return fixture.web; },
}));
vi.mock("./voice/audio", async (original) => ({
  ...await original<typeof import("./voice/audio")>(),
  storeBlob: async (blob: Blob) => { fixture.stored.push(blob); return "/uploads/pasted.png"; },
}));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it.each([[true, true, true], [true, true, false], [true, false, true], [false, true, true]])("pastes event text and image without a clipboard permission read (web=%s, items=%s, files=%s)", async (web, hasItems, hasFiles) => {
  fixture.web = web;
  fixture.stored = [];
  const read = vi.fn(() => Promise.reject(new Error("Clipboard permission read")));
  const readText = vi.fn(() => Promise.reject(new Error("Clipboard permission read")));
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { read, readText } });
  const sent: string[] = [];
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    if (command === "chat_send_text") {
      sent.push(String(args?.text));
      return { send_id: "sent", tab_id: "paste", state: "delivered", ledger_id: "ledger", sequence: 1, retained_from: 0 } as T;
    }
    if (["read_clipboard_files", "attachment_path_kinds", "delayed_send_list", "cron_list", "list_agent_providers"].includes(command)) return [] as T;
    return { items: [], receiverTargets: {} } as T;
  });
  __setListenForTest(async () => () => {});
  const state: ExtendedState = {
    workspaces: [], tabs: {}, teams: [], active_workspace_id: null, focused_tab_id: null,
    notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [],
  };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  function Harness() {
    const chatRootRef = useRef<HTMLDivElement>(null);
    const paste = useAttachments({ chatRootRef, enabled: true, onError: (error) => { throw new Error(error); } });
    return <div ref={chatRootRef}><MuxStateStoreContext.Provider value={createMuxStateStore(() => state)}>
      <ChatComposer tabId="paste" tabProvider={null} currentPtyId={null} pendingPrompt={null}
        attachments={paste.attachments} addAttachments={paste.addAttachments} removeAttachment={paste.removeAttachment}
        clearAttachments={paste.clearAttachments} handlePasteEvent={paste.handlePasteEvent} inlineAttachmentSink={paste.inlineSinkRef}
        onWillSendMessage={() => {}} addOptimistic={() => "sent"} updateOptimistic={() => {}}
        observeChatSendStatus={() => {}} showComposerError={(error) => { throw new Error(error); }} />
    </MuxStateStoreContext.Provider></div>;
  }
  try {
    await act(async () => root.render(<Harness />));
    const editor = host.querySelector<HTMLTextAreaElement>('textarea')!;
    editor.focus();
    const image = new File(["image"], "pasted.png", { type: "image/png" });
    const text = hasItems ? "Inspect this image" : "Inspect this image\n".repeat(100);
    const event = Object.assign(new Event("paste", { bubbles: true, cancelable: true }), {
      clipboardData: {
        items: hasItems ? [{ kind: "file", type: "image/png", getAsFile: () => image }] : [], files: hasFiles ? [new File(["image"], "pasted.png", { type: "image/png" })] : [],
        getData: (type: string) => type === "text/plain" ? text : "",
      },
    });
    await act(async () => editor.dispatchEvent(event));
    expect(fixture.stored).toHaveLength(1);
    expect(fixture.stored[0].type).toBe(image.type);
    expect(await fixture.stored[0].text()).toBe("image");
    expect(editor.value).toContain("Inspect this image");
    expect(capsuleSpans(editor.value)).toHaveLength(1);
    await act(async () => editor.dispatchEvent(new KeyboardEvent("keydown", { code: "Enter", key: "Enter", bubbles: true })));
    expect(sent).toHaveLength(1);
    expect(sent[0]).toContain("Inspect this image");
    expect(parseAttachmentMarkers(sent[0]).attachments.map(({ path }) => path)).toEqual(["/uploads/pasted.png"]);
    expect(sent[0].split("/uploads/pasted.png")).toHaveLength(2);
    expect(read).not.toHaveBeenCalled();
    expect(readText).not.toHaveBeenCalled();
  } finally {
    await act(async () => root.unmount()); host.remove();
    __setInvokeForTest(null); __setListenForTest(null);
    fixture.web = true;
    Reflect.deleteProperty(navigator, "clipboard");
  }
});
