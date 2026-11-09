import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import { MuxStateStoreContext, MultiplexerActionsContext, type MultiplexerActionsContextType } from "../../store/multiplexer/context";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { parseAttachmentMarkers } from "@codeterm/chat-core";
import { capsuleSpans } from "./composerCapsules";
import { ChatComposer } from "./ChatComposer";
import { DraftStorageFixture } from "../../test/draftStorage.testdata";
import { useAttachments } from "./useAttachments";
import { nativeBridge } from "../../lib/nativeBridge";
import { TabSessionChat } from "./TabSessionChat";
import { clearTranscriptWindows } from "./transcriptStore";

const fixture = vi.hoisted(() => ({ web: true, stored: [] as Blob[] }));
vi.mock("../../lib/webApi", async (original) => ({
  ...await original<typeof import("../../lib/webApi")>(),
  get IS_WEB() { return fixture.web; },
}));
vi.mock("./voice/audio", async (original) => ({
  ...await original<typeof import("./voice/audio")>(),
  storeBlob: async (blob: Blob) => { fixture.stored.push(blob); return "/uploads/pasted.png"; },
}));
vi.mock("../../hooks/useAgentProviders", () => ({ useAgentProviders: () => ({ providers: [], loading: false, error: null }) }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it.each([
  [true, true, true, true], [true, true, false, true], [true, false, true, true], [false, true, true, true],
  [false, true, true, false], [false, true, false, false], [false, false, true, false],
])("pastes one event image without a clipboard permission read (web=%s, items=%s, files=%s, text=%s)", async (web, hasItems, hasFiles, hasText) => {
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
    workspaces: [], tabs: { paste: { id: "paste", pty_id: null, tab_type: "Terminal", title: "Composer", cwd: null } }, teams: [], active_workspace_id: null, focused_tab_id: null,
    notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [],
  };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  function Harness() {
    const chatRootRef = useRef<HTMLDivElement>(null);
    const paste = useAttachments({ chatRootRef, enabled: true, onError: (error) => { throw new Error(error); } });
    return <div ref={chatRootRef}><DraftStorageFixture><MuxStateStoreContext.Provider value={createMuxStateStore(() => state)}>
      <ChatComposer tabId="paste" tabProvider={null} currentPtyId={null} pendingPrompt={null}
        attachments={paste.attachments} addAttachments={paste.addAttachments} removeAttachment={paste.removeAttachment}
        clearAttachments={paste.clearAttachments} handlePasteEvent={paste.handlePasteEvent} inlineAttachmentSink={paste.inlineSinkRef}
        onWillSendMessage={() => {}} addOptimistic={() => "sent"} updateOptimistic={() => {}}
        observeChatSendStatus={() => {}} showComposerError={(error) => { throw new Error(error); }} />
    </MuxStateStoreContext.Provider></DraftStorageFixture></div>;
  }
  try {
    await act(async () => root.render(<Harness />));
    const editor = host.querySelector<HTMLTextAreaElement>('textarea')!;
    editor.focus();
    const image = new File(["image"], "pasted.png", { type: "image/png" });
    for (const modifier of ["ctrlKey", "metaKey"]) {
      const shortcut = new KeyboardEvent("keydown", { code: "KeyV", key: "v", [modifier]: true, bubbles: true, cancelable: true });
      await act(async () => editor.dispatchEvent(shortcut));
      expect(shortcut.defaultPrevented).toBe(false);
    }
    const text = hasText ? (hasItems ? "Inspect this image" : "Inspect this image\n".repeat(100)) : "";
    const event = new ClipboardEvent("paste", { bubbles: true, cancelable: true,
      clipboardData: {
        items: hasItems ? [{ kind: "file", type: "image/png", getAsFile: () => image }] : [], files: hasFiles ? [new File(["image"], "pasted.png", { type: "image/png" })] : [],
        getData: (type: string) => type === "text/plain" ? text : "",
      } as unknown as DataTransfer,
    });
    await act(async () => editor.dispatchEvent(event));
    expect(fixture.stored).toHaveLength(1);
    expect(fixture.stored[0].type).toBe(image.type);
    expect(await fixture.stored[0].text()).toBe("image");
    expect(event.defaultPrevented).toBe(true);
    expect(editor.value.includes("Inspect this image")).toBe(hasText);
    expect(capsuleSpans(editor.value)).toHaveLength(1);
    await act(async () => editor.dispatchEvent(new KeyboardEvent("keydown", { code: "Enter", key: "Enter", bubbles: true })));
    expect(sent).toHaveLength(1);
    expect(sent[0].includes("Inspect this image")).toBe(hasText);
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

it("keeps image paste available while a reactivated chat reacquires its transcript", async () => {
  fixture.web = false;
  fixture.stored = [];
  let starts = 0;
  let finishAttach!: (value: unknown) => void;
  const pendingAttach = new Promise(resolve => { finishAttach = resolve; });
  const snapshot = { messages: [], pending: false, source_session_id: "paste-session", session_binding_nonce: "paste-nonce" };
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    if (command === "chat_start") {
      if (++starts > 1) return await pendingAttach as T;
      return { ...snapshot, subscription_id: args?.subscriptionId } as T;
    }
    if (["read_clipboard_files", "attachment_path_kinds", "delayed_send_list", "cron_list", "list_agent_providers", "chat_send_status"].includes(command)) return [] as T;
    if (command === "plugin_manager_snapshot") return { plugins: [], channels: [], diff: [], marketplace: [] } as T;
    return { items: [], receiverTargets: {} } as T;
  });
  __setListenForTest(async () => () => {});
  const state: ExtendedState = {
    workspaces: [], tabs: { reactivated: { id: "reactivated", title: "Fixture", tab_type: "Terminal", cwd: null,
      pty_id: "paste-pty", backing: { kind: "pty" }, agent_session_id: "paste-session", session_binding_nonce: "paste-nonce",
      boot_stage: "agent_ready", agent_runtime_state: { state: "loaded", epoch: 1 } } }, teams: [], active_workspace_id: null,
    focused_tab_id: "reactivated", notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [],
  };
  const store = createMuxStateStore(() => state);
  const actions = new Proxy({}, { get: () => () => {} }) as MultiplexerActionsContextType;
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const render = (isVisible: boolean) => act(async () => root.render(<DraftStorageFixture>
    <MuxStateStoreContext.Provider value={store}><MultiplexerActionsContext.Provider value={actions}>
      <TabSessionChat tabId="reactivated" isVisible={isVisible} />
    </MultiplexerActionsContext.Provider></MuxStateStoreContext.Provider>
  </DraftStorageFixture>));
  try {
    await render(true);
    expect(starts).toBe(1);
    await render(false);
    await render(true);
    expect(starts).toBe(2);
    const editor = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message composer"]')!;
    expect(editor).not.toBeNull();
    editor.focus();
    const image = new File(["reactivated image"], "reactivated.png", { type: "image/png" });
    const data = new DataTransfer(); data.items.add(image);
    const event = new ClipboardEvent("paste", { bubbles: true, cancelable: true, clipboardData: data });
    await act(async () => editor.dispatchEvent(event));
    expect(fixture.stored).toHaveLength(1);
    expect(await fixture.stored[0].text()).toBe("reactivated image");
    expect(capsuleSpans(editor.value)).toHaveLength(1);
    const text = new DataTransfer(); text.setData("text/plain", "ordinary text");
    const textEvent = new ClipboardEvent("paste", { bubbles: true, cancelable: true, clipboardData: text });
    await act(async () => editor.dispatchEvent(textEvent));
    expect(textEvent.defaultPrevented).toBe(false);
    expect(fixture.stored).toHaveLength(1);
  } finally {
    finishAttach(snapshot);
    await act(async () => root.unmount()); host.remove();
    clearTranscriptWindows();
    __setInvokeForTest(null); __setListenForTest(null);
    fixture.web = true;
  }
});

it("surfaces a native attachment refusal without adding the source path", async () => {
  fixture.web = false;
  const errors: string[] = [];
  const originalMaterialize = nativeBridge.materializeDroppedFiles;
  nativeBridge.materializeDroppedFiles = async () => { throw new Error("Empty file"); };
  __setInvokeForTest(async <T,>(command: string): Promise<T> =>
    (command === "read_clipboard_files" ? ["/temporary/recording.mov"] : []) as T);
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  let paths: string[] = [];
  function Harness() {
    const chatRootRef = useRef<HTMLDivElement>(null);
    const paste = useAttachments({ chatRootRef, enabled: true, onError: message => errors.push(message) });
    paths = paste.attachments;
    return <div ref={chatRootRef}><textarea onPaste={event => { void paste.handlePasteEvent(event.nativeEvent); }} /></div>;
  }
  try {
    await act(async () => root.render(<Harness />));
    const editor = host.querySelector("textarea")!;
    editor.focus();
    const event = Object.assign(new Event("paste", { bubbles: true, cancelable: true }), {
      clipboardData: { items: [], files: [], types: [], getData: () => "" },
    });
    await act(async () => editor.dispatchEvent(event));
    expect(errors).toEqual(["Empty file"]);
    expect(paths).toEqual([]);
    expect(event.defaultPrevented).toBe(true);
  } finally {
    await act(async () => root.unmount()); host.remove();
    nativeBridge.materializeDroppedFiles = originalMaterialize;
    __setInvokeForTest(null);
    fixture.web = true;
  }
});
