import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { MessageBubble } from "@codeterm/chat-ui";
import { parseAttachmentMarkers } from "@codeterm/chat-core";
import { configureDaemonFixture } from "../../test/daemonTransport.testdata";
import { DraftStorageFixture } from "../../test/draftStorage.testdata";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { ChatComposer } from "./ChatComposer";
import { ChatDeliveryState } from "./ChatDeliveryState";
import { ComposerIssuesProvider } from "./ComposerIssuesContext";
import { DroppedUploadNotice } from "./DroppedUploadNotice";
import { capsuleSpans, capsuleToken } from "./composerCapsules";
import { deliveryError, deliveryState } from "./chatDeliveryModel";
import { reconcileTrackedChatMessages } from "./chatSendStatus";
import { MemoryChatSendRepository } from "./chatSendRepository.testSupport";
import { addChatSend, ensureChatSendTabHydrated, forgetChatSendsInMemory, readChatSends, useChatSends,
  __setChatSendRepositoryForTest } from "./chatSendStore";

const uploads = vi.hoisted(() => ({ store: undefined as undefined | ((blob: Blob, name: string) => Promise<string>) }));
vi.mock("./voice/audio", async (original) => ({
  ...await original<typeof import("./voice/audio")>(),
  storeBlob: (blob: Blob, name: string) => uploads.store!(blob, name),
}));

beforeEach(() => {
  configureDaemonFixture();
  __setChatSendRepositoryForTest(new MemoryChatSendRepository());
});
afterEach(() => { __setInvokeForTest(null); __setListenForTest(null); });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

async function mount(tabId: string, requestFailure?: "admission" | "network" | "preflight", sendGate?: Promise<void>) {
  const sent: string[] = [];
  let bubbles = 0;
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    if (command === "chat_send_text") {
      sent.push(String(args?.text));
      await sendGate;
      if (requestFailure === "admission") throw new Error("user send capacity: 32 unsettled admissions");
      if (requestFailure === "network") throw new TypeError("send connection interrupted");
      return { tab_id: tabId, send_id: "host-send", correlationId: "bubble", state: requestFailure === "preflight" ? "failed" : "delivered",
        error: requestFailure === "preflight" ? "runtime_preflight_before_write: binding changed" : null,
        ledger_id: "ledger", sequence: 1, retained_from: 0 } as T;
    }
    if (command === "mesh_transfer_attachment") throw new Error("attachment copy refused");
    if (command === "chat_send_admissions") return (args?.clientTokens as string[]).map(correlationId => ({ correlationId, evidence: "absent" })) as T;
    if (["chat_send_status", "chat_send_dismissals", "delayed_send_list", "cron_list", "list_agent_providers"].includes(command)) return [] as T;
    return { items: [], receiverTargets: {} } as T;
  });
  __setListenForTest(async () => () => {});
  const state = { workspaces: [], tabs: { [tabId]: { id: tabId, pty_id: null, tab_type: "Terminal", title: "Images", cwd: null } },
    teams: [], notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [],
    active_workspace_id: null, focused_tab_id: null } as ExtendedState;
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const store = createMuxStateStore(() => state);
  function Harness() {
    const sends = useChatSends(tabId);
    return <DraftStorageFixture><MuxStateStoreContext.Provider value={store}><ComposerIssuesProvider>
      {sends.map(send => <MessageBubble key={send.id} message={{ id: send.id, type: "user", content: send.content }}
        userFooter={<><ChatDeliveryState state={deliveryState(send)} error={deliveryError(send)} />
          <DroppedUploadNotice tabId={tabId} id={send.id} names={send.droppedUploads} /></>} />)}
      <ChatComposer tabId={tabId} tabProvider={null} currentPtyId={null} chatBackend pendingPrompt={null}
        attachments={[]} addAttachments={() => {}} removeAttachment={() => {}} clearAttachments={() => {}}
        handlePasteEvent={() => {}} onWillSendMessage={() => {}}
        addOptimistic={content => { const id = ++bubbles === 1 ? "bubble" : `bubble-${bubbles}`; addChatSend(tabId, { id, correlationId: id, content, sentAtMs: bubbles }); return id; }}
        updateOptimistic={() => {}} observeChatSendStatus={() => {}} showComposerError={error => { throw new Error(error); }} />
    </ComposerIssuesProvider></MuxStateStoreContext.Provider></DraftStorageFixture>;
  }
  await act(async () => root.render(<Harness />));
  const editor = host.querySelector<HTMLTextAreaElement>('[data-testid="chat-composer"]')!;
  return {
    host, sent,
    async text(value: string) {
      await act(async () => {
        Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(editor, value);
        editor.dispatchEvent(new Event("input", { bubbles: true }));
      });
    },
    async pick(files: File[]) {
      const picker = host.querySelector<HTMLInputElement>('[data-testid="composer-attach-input"]')!;
      Object.defineProperty(picker, "files", { configurable: true, value: files });
      await act(async () => picker.dispatchEvent(new Event("change", { bubbles: true })));
    },
    async send() { await act(async () => editor.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", code: "Enter", bubbles: true }))); },
    async dispose() { await act(async () => root.unmount()); host.remove(); },
  };
}

it("waits for two screenshot-sized uploads and sends their Unicode paths through the composer once", async () => {
  const tabId = "two-images";
  const pending: Array<{ name: string; finish: (path: string) => void }> = [];
  uploads.store = async (blob, name) => {
    expect(blob.size).toBe(1_600_000);
    return new Promise<string>(finish => pending.push({ name, finish }));
  };
  const view = await mount(tabId);
  try {
    await view.text("inspect both screenshots");
    await view.pick([0, 1].map(index => new File([new Uint8Array(1_600_000)], `Снимок экрана ${index}\u202fPM.png`, { type: "image/png" })));
    await view.send();
    expect(pending).toHaveLength(2);
    expect(view.sent).toHaveLength(0);
    const paths = pending.map(({ name }) => `/var/folders/temp/${name}`);
    expect(paths[0]).toContain("Снимок экрана");
    expect(paths[0]).toContain("\u202f");
    await act(async () => pending.forEach((upload, index) => upload.finish(paths[index])));
    expect(view.sent).toHaveLength(1);
    expect(parseAttachmentMarkers(view.sent[0]).attachments.map(image => image.path)).toEqual(paths);
    expect(capsuleSpans(view.sent[0])).toHaveLength(0);
    expect(readChatSends(tabId)).toHaveLength(1);
  } finally { await view.dispose(); }
});

it.each([false, true])("keeps distinct images with reused capsule labels while a send is pending (upload pending: %s)", async pending => {
  const tabId = `distinct-images-${pending}`;
  let finishSend!: () => void;
  const sendGate = new Promise<void>(resolve => { finishSend = resolve; });
  const uploadsToFinish: Array<() => void> = [];
  let uploadIndex = 0;
  uploads.store = async () => {
    const path = `/opaque/image-${++uploadIndex}.png`;
    if (pending) await new Promise<void>(resolve => uploadsToFinish.push(resolve));
    return path;
  };
  const view = await mount(tabId, undefined, sendGate);
  try {
    for (let index = 0; index < 2; index++) {
      await view.pick([new File(["pixels"], "shot.png", { type: "image/png" })]);
      const editor = view.host.querySelector<HTMLTextAreaElement>("textarea")!;
      await view.text(`${editor.value.trim()} inspect`);
      await view.send();
      expect(editor.value).toBe("");
    }
    expect(readChatSends(tabId).map(send => send.id)).toEqual(["bubble", "bubble-2"]);
    await act(async () => { uploadsToFinish.forEach(finish => finish()); });
    expect(view.sent.map(text => parseAttachmentMarkers(text).attachments.map(image => image.path)))
      .toEqual([["/opaque/image-1.png"], ["/opaque/image-2.png"]]);
  } finally {
    await act(async () => { uploadsToFinish.forEach(finish => finish()); finishSend(); });
    await view.dispose();
  }
});

it.each([
  ["upload", "image file could not be read"],
  ["resolution", 'Attachment "image1" is unavailable'],
  ["handoff", "attachment copy refused"],
  ["admission", "user send capacity: 32 unsettled admissions"],
  ["network", "send connection interrupted"],
  ["preflight", "binding changed"],
] as const)("retains a visible %s failure through reload without sending an image label", async (stage, reason) => {
  const tabId = stage === "handoff" ? "mesh:eight/image-failure" : `image-${stage}`;
  uploads.store = stage === "upload" ? async () => { throw new Error(reason); } : async () => "/var/folders/temp/shot.png";
  const view = await mount(tabId, stage === "admission" || stage === "network" || stage === "preflight" ? stage : undefined);
  try {
    if (stage === "resolution") await view.text(capsuleToken("image1"));
    else await view.pick([new File(["pixels"], "shot.png", { type: "image/png" })]);
    await view.send();
    if (stage === "network") await act(async () => { await reconcileTrackedChatMessages(tabId); });
    expect(view.sent).toHaveLength(["admission", "network", "preflight"].includes(stage) ? 1 : 0);
    for (const sent of view.sent) {
      expect(capsuleSpans(sent)).toHaveLength(0);
      expect(parseAttachmentMarkers(sent).attachments).toHaveLength(1);
    }
    await act(async () => { forgetChatSendsInMemory(); await ensureChatSendTabHydrated(tabId); });
    const send = readChatSends(tabId)[0];
    expect(deliveryState(send)).toBe("failed");
    expect(deliveryError(send)).toContain(reason);
    expect(view.host.querySelector('[role="status"][data-state="failed"]')?.getAttribute("aria-label")).toContain(reason);
    if (["upload", "resolution", "handoff"].includes(stage)) expect(send.draft).toBeDefined();
  } finally { await view.dispose(); }
});
