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
import { parseAttachmentMarkers } from "@codeterm/chat-core";
import { createTabsTransport } from "../../lib/transport/tabs";
import type { HttpMethod } from "../../lib/transportClient";
import { runtimePlan, runtimeResponse, runtimeReduce } from "../../../plugins/provider-acp/src/runtime";
import type { RuntimeContext, Operation } from "@codeterm/plugin-sdk";

vi.mock("../../lib/frontendUpdate/draftStorage", () => ({
  createDraftStorage: () => ({ read: async () => new Map<string, string>(), write: async () => {} }),
}));

beforeEach(() => configureDaemonFixture());
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const readable = { filePaths: true, inlineText: true, imagePaths: true };
const inline = { filePaths: false, inlineText: true, imagePaths: false };
const images = { filePaths: false, inlineText: true, imagePaths: true };

vi.mock("./voice/audio", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./voice/audio")>()),
  storeBlob: async () => "/opaque/picked.png",
}));

it("sends the image selected in Attach file on a runtime tab", async () => {
  setAgentDescriptors([{ id: "opaque", attachments: images } as ProviderDescriptor]);
  const sent: string[] = [];
  const initial: RuntimeContext = { tabId: "picked-tab", cwd: "/opaque", hostVersion: "1.19.1", epoch: 1,
    bindingNonce: "picker-nonce", settings: {}, providerSettings: {}, claimedSends: [], activeTurns: [], threadParents: [] };
  const init: Operation = { kind: "initialize" };
  runtimeResponse(initial, init, runtimePlan(initial, init).state, { kind: "success", value: { protocolVersion: 1, agentCapabilities: { promptCapabilities: { image: true } }, authMethods: [] } });
  const open: Operation = { kind: "openThread", instructions: "", settings: {} };
  runtimeResponse(initial, open, runtimePlan(initial, open).state, { kind: "success", value: { sessionId: "picker-session" } });
  const transport = createTabsTransport({ async json<T>(method: HttpMethod, path: string, body?: unknown): Promise<T> {
    expect(method).toBe("POST");
    expect(path).toBe("/api/tabs/picked-tab/chat-send");
    const payload = body as { text: string; client_token: string };
    sent.push(payload.text);
    const input = { text: payload.text, attachments: parseAttachmentMarkers(payload.text).attachments.map(item => ({
      id: `attachment:${item.path}`, mimeType: "image/png", path: item.path, data: "cGl4ZWxz",
    })) };
    expect(input.attachments).toHaveLength(1);
    const context = { ...initial, threadId: "picker-session", claimedSends: [{ threadId: "picker-session", sendId: payload.client_token, input }] };
    const op: Operation = { kind: "submitTurn", threadId: "picker-session", sendId: payload.client_token, input, settings: {} };
    const plan = runtimePlan(context, op);
    expect(plan.action).toMatchObject({ kind: "request", call: { method: "session/prompt", params: {
      prompt: [{ type: "text", text: payload.text }, { type: "image", data: "cGl4ZWxz", mimeType: "image/png" }],
    } } });
    const reduction = runtimeReduce(context, null, [{ kind: "notification", method: "session/update", params: {
      sessionId: "picker-session", update: { sessionUpdate: "agent_message_chunk", content: { type: "text", text: "seen" } },
    } }]);
    const users = reduction.events.filter(event => event.kind === "chatItem" && event.item.type === "user");
    expect(users).toHaveLength(1);
    expect(users[0]).toMatchObject({ item: { content: payload.text, attachments: input.attachments } });
    expect(reduction.events).toContainEqual(expect.objectContaining({ kind: "sendEvidence", sendId: payload.client_token }));
    expect(JSON.stringify(reduction.state)).not.toContain("cGl4ZWxz");
    return { send_id: "picked", tab_id: "picked-tab", state: "delivered", ledger_id: "ledger", sequence: 1, retained_from: 0 } as T;
  } });
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    if (command === "chat_send_text") {
      return await transport.chat_send_text(args ?? {}) as T;
    }
    if (["delayed_send_list", "cron_list"].includes(command)) return [] as T;
    return { items: [], receiverTargets: {} } as T;
  });
  __setListenForTest(async () => () => {});
  const state = { workspaces: [], tabs: { "picked-tab": { id: "picked-tab", pty_id: null, tab_type: "Terminal", title: "Composer", cwd: null } }, teams: [], notifications: [], tabStatuses: {}, toasts: [],
    activeTabIds: {}, generation: 1, pendingCloses: [], active_workspace_id: null, focused_tab_id: null } as ExtendedState;
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<MuxStateStoreContext.Provider value={createMuxStateStore(() => state)}><ComposerIssuesProvider>
      <ChatComposer tabId="picked-tab" tabProvider="opaque" currentPtyId={null} chatBackend pendingPrompt={null}
        attachments={[]} addAttachments={() => {}} removeAttachment={() => {}} clearAttachments={() => {}}
        handlePasteEvent={() => {}} onWillSendMessage={() => {}} addOptimistic={() => "picked"}
        updateOptimistic={() => {}} observeChatSendStatus={() => {}} showComposerError={(error) => { throw new Error(error); }} />
    </ComposerIssuesProvider></MuxStateStoreContext.Provider>));
    const picker = host.querySelector<HTMLInputElement>('[data-testid="composer-attach-input"]')!;
    Object.defineProperty(picker, "files", { value: [new File(["pixels"], "picked.png", { type: "image/png" })] });
    await act(async () => picker.dispatchEvent(new Event("change", { bubbles: true })));
    const editor = host.querySelector<HTMLTextAreaElement>("textarea")!;
    expect(editor.value).toContain("image");
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(editor, `${editor.value} inspect`);
      editor.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => host.querySelector<HTMLButtonElement>('[data-testid="composer-send-button"]')!.dispatchEvent(new PointerEvent("pointerup", { button: 0, bubbles: true })));
    expect(sent).toHaveLength(1);
    expect(parseAttachmentMarkers(sent[0]).attachments.map(item => item.path)).toEqual(["/opaque/picked.png"]);
  } finally { await act(async () => root.unmount()); host.remove(); setAgentDescriptors([]); }
});

it.each([
  ["file", "/opaque/note.txt", inline, false],
  ["image", "/opaque/photo.png", inline, false],
  ["file", "/opaque/note.txt", readable, true],
  ["image", "/opaque/photo.png", readable, true],
  ["unknown", "/opaque/note.txt", undefined, false],
  ["image-only runtime image", "/opaque/photo.png", images, true],
  ["image-only runtime file", "/opaque/note.txt", images, false],
  ["image-only runtime folder", "/opaque/folder", images, false],
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
  const state = { workspaces: [], tabs: { capability: { id: "capability", pty_id: null, tab_type: "Terminal", title: "Composer", cwd: null } }, teams: [], notifications: [], tabStatuses: {}, toasts: [],
    activeTabIds: {}, generation: 1, pendingCloses: [], active_workspace_id: null, focused_tab_id: null } as ExtendedState;
  const store = createMuxStateStore(() => state);
  const clear = vi.fn();
  const optimistic = vi.fn(() => "sent");
  const updateOptimistic = vi.fn();
  const error = vi.fn();
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<MuxStateStoreContext.Provider value={store}><ComposerIssuesProvider>
      <ChatComposer tabId="capability" tabProvider="opaque" currentPtyId={null} chatBackend pendingPrompt={null}
        attachments={[path]} addAttachments={() => {}} removeAttachment={() => {}} clearAttachments={clear}
        handlePasteEvent={() => {}} onWillSendMessage={() => {}} addOptimistic={optimistic}
        updateOptimistic={updateOptimistic} observeChatSendStatus={() => {}} showComposerError={error} />
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
    if (allowed) {
      expect(parseAttachmentMarkers(sent[0]).attachments.map(item => item.path)).toEqual([path]);
      expect(updateOptimistic).toHaveBeenCalledWith("sent", sent[0]);
    }
    else {
      expect(editor.value).toBe("inspect opaque");
      expect(editor.getAttribute("aria-invalid")).toBe("true");
      expect(error).toHaveBeenCalledOnce();
    }
  } finally {
    await act(async () => root.unmount()); host.remove(); setAgentDescriptors([]);
  }
});
