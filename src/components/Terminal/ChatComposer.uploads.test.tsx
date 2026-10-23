import { ComposerIssuesProvider } from "./ComposerIssuesContext";
import { AgentRuntimeBanner } from "./AgentRuntimeBanner";
import { beforeEach as beforeDaemonCase } from "vitest";
import { configureDaemonFixture } from "../../test/daemonTransport.testdata";
beforeDaemonCase(() => configureDaemonFixture());
import { Blob as BrowserBlob, File as BrowserFile, FormData as BrowserFormData } from "happy-dom";
import { act, useState } from "react";
import { addChatSend, useChatSends, removeChatSend } from "./chatSendStore";
import { DroppedUploadNotice } from "./DroppedUploadNotice";
import { ChatDeliveryState } from "./ChatDeliveryState";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { LocalArtifactScope, MessageBubble } from "@codeterm/chat-ui";
import { parseAttachmentMarkers } from "@codeterm/chat-core";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { configureLocalPreviews } from "../../lib/configureLocalPreviews";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { capsuleSpans } from "./composerCapsules";
import { ChatComposer } from "./ChatComposer";
import { prepareBrowserUpload } from "./browserImageUpload";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const path = "/home/owner/.domios-dev/uploads/id-photo.png";
const pixels = "data:image/png;base64,iVBORw0KGgo=";

it.each(["success", "HEIC", "pending", "HTTP 503", "NotReadableError", "HTTP 400", "pending HTTP 400"])("web picker sends immediately and omits failed images without a popup: %s", async (failure) => {
  vi.stubGlobal("Blob", BrowserBlob);
  vi.stubGlobal("File", BrowserFile);
  vi.stubGlobal("FormData", BrowserFormData);
  if (failure === "HEIC") {
    vi.stubGlobal("createImageBitmap", vi.fn(async () => ({ width: 2, height: 2, close: vi.fn() })));
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({ drawImage: vi.fn() } as unknown as CanvasRenderingContext2D);
    vi.spyOn(HTMLCanvasElement.prototype, "toBlob").mockImplementation(callback => callback(new Blob(["image"], { type: "image/jpeg" })));
  }
  const sent: string[] = [];
  const previews: string[] = [];
  let requests = 0;
  let reads = 0;
  let completeUpload!: () => void;
  const fetch = vi.fn(async (_url: unknown, options?: RequestInit) => {
    expect(String(_url)).toContain("/api/attachments/upload");
    const body = options?.body as FormData;
    const file = body.get("file") as File;
    expect(body).toBeInstanceOf(FormData);
    expect(file).toBeInstanceOf(Blob);
    expect(file.type).toBe(failure === "HEIC" ? "image/jpeg" : "image/png");
    expect(file.name).toBe(failure === "HEIC" ? "photo.jpg" : "photo.png");
    expect(file.size).toBe(failure.includes("400") ? 0 : 5);
    if (failure === "NotReadableError" && ++reads === 1) throw new DOMException("unreadable", "NotReadableError");
    if (++requests === 1 && failure === "HTTP 503") return new Response("busy", { status: 503 });
    if (failure.startsWith("pending")) await new Promise<void>(resolve => { completeUpload = resolve; });
    if (failure.includes("400")) return Response.json({ error: { code: "empty_attachment", message: "Attachment is empty" } }, { status: 400 });
    return Response.json({ path, size: 5 });
  });
  vi.stubGlobal("fetch", fetch);
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    if (command === "chat_send_text") {
      sent.push(String(args?.text));
      return { send_id: "sent", tab_id: "picker", state: "delivered", ledger_id: "ledger", sequence: 1, retained_from: 0 } as T;
    }
    if (command === "read_local_preview_file") {
      previews.push(String(args?.path));
      return { data: pixels, media_type: "image/png" } as T;
    }
    if (["list_agent_providers", "delayed_send_list", "cron_list"].includes(command)) return [] as T;
    return { items: [], receiverTargets: {} } as T;
  });
  __setListenForTest(async () => () => {});
  configureLocalPreviews();
  const state: ExtendedState = {
    workspaces: [], tabs: {}, teams: [], active_workspace_id: null, focused_tab_id: null,
    notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [],
  };
  const store = createMuxStateStore(() => state);
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  function Harness() {
    const [attachments, setAttachments] = useState<string[]>([]);
    const [bubble, setBubble] = useState("");
    const sends = useChatSends("picker");
    return <MuxStateStoreContext.Provider value={store}><LocalArtifactScope tabId="picker">
      {bubble && <div role="region" aria-label="Sent message"><MessageBubble message={{ id: "sent", type: "user", content: bubble }} userFooter={<><ChatDeliveryState state="sending" /><DroppedUploadNotice tabId="picker" id="sent" names={sends.find(send => send.id === "sent")?.droppedUploads} /></>} /></div>}
      <ComposerIssuesProvider><AgentRuntimeBanner state={null} onRetry={() => {}} /><ChatComposer tabId="picker" tabProvider={null} currentPtyId={null} pendingPrompt={null}
        attachments={attachments} addAttachments={(paths) => setAttachments((current) => [...current, ...paths])}
        removeAttachment={(path) => setAttachments((current) => current.filter((item) => item !== path))}
        clearAttachments={() => setAttachments([])} handlePasteEvent={() => {}} onWillSendMessage={() => {}}
        addOptimistic={(content) => { addChatSend("picker", { id: "sent", content, sentAtMs: Date.now() }); setBubble(content); return "sent"; }} updateOptimistic={(_id, content) => setBubble(content)}
        observeChatSendStatus={() => {}} showComposerError={(error) => { throw new Error(error); }} /></ComposerIssuesProvider>
    </LocalArtifactScope></MuxStateStoreContext.Provider>;
  }
  try {
    await act(async () => root.render(<Harness />));
    const editor = host.querySelector<HTMLTextAreaElement>('[data-testid="chat-composer"]')!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(editor, "Please inspect here");
      editor.dispatchEvent(new Event("input", { bubbles: true }));
      editor.setSelectionRange(15, 15);
    });
    const input = host.querySelector<HTMLInputElement>('[data-testid="composer-attach-input"]')!;
    const file = new File([failure.includes("400") ? "" : "image"], failure === "HEIC" ? "photo.heic" : "photo.png", { type: failure === "HEIC" ? "image/heic" : "image/png" });
    Object.defineProperty(input, "files", { value: [file] });
    await act(async () => input.dispatchEvent(new Event("change", { bubbles: true })));
    expect(fetch.mock.calls.length, host.textContent ?? "").toBe(1);
    if (failure.startsWith("pending")) {
      expect(host.querySelector('[role="status"][aria-label="Uploading photo.png"]')).not.toBeNull();
      expect(host.querySelector('[aria-label="Remove attachment photo.png"]')).not.toBeNull();
      expect(editor.disabled).toBe(false);
    }
    if (failure === "HEIC") await expect(fetch.mock.results[0].value).resolves.toBeInstanceOf(Response);
    if (failure === "HTTP 503" || failure === "NotReadableError") {
      const notice = host.querySelector('[role="status"][aria-label^="Failed attachment"]');
      expect(notice).not.toBeNull();
      expect(host.querySelector('[role="group"][aria-label="Message composer"]')?.contains(notice)).toBe(true);
      expect(editor.value).not.toContain(file.name);
    }
    if (failure === "HTTP 400") {
      expect(host.querySelector('[role="status"][aria-label^="Failed attachment"]')?.getAttribute("title")).toBe("Attachment is empty");
      expect(host.querySelector('[aria-label="Retry attachment photo.png"]')).toBeNull();
    }
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(editor, `${editor.value} after upload`);
      editor.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(capsuleSpans(editor.value)).toHaveLength(1);
    await act(async () => editor.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", code: "Enter", bubbles: true })));
    expect(editor.value).toBe("");
    if (failure.startsWith("pending")) {
      expect(sent).toHaveLength(0);
      expect(host.querySelector('[role="status"]')).not.toBeNull();
      await act(async () => {
        Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(editor, "next message");
        editor.dispatchEvent(new Event("input", { bubbles: true }));
      });
      await act(async () => completeUpload());
      expect(editor.value).toBe("next message");
    }
    expect(sent).toHaveLength(1);
    if (failure.includes("400")) {
      expect(host.querySelector('[aria-label="Retry omitted attachments"]')).toBeNull();
      expect(host.querySelector('[role="note"]')?.textContent).toContain("Attachment is empty");
      expect(host.textContent).not.toContain("/api/attachments/upload");
      expect(parseAttachmentMarkers(sent[0]).attachments).toEqual([]);
      return;
    }
    const row = host.querySelector('[data-testid="user-bubble-row"]')!;
    expect(row.querySelector('[aria-label="User message"]')?.parentElement).toBe(row);
    expect(row.querySelector('[role="status"]')?.parentElement).toBe(row);
    expect(host.querySelector('[role="dialog"]')).toBeNull();
    if (failure !== "success" && failure !== "HEIC" && failure !== "pending") {
      expect(parseAttachmentMarkers(sent[0]).attachments).toEqual([]);
      expect(host.querySelector('button[aria-label="Retry omitted attachments"]')).not.toBeNull();
      expect(row.querySelector('[role="note"]')).not.toBeNull();
      await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="Retry omitted attachments"]')!.click());
      expect(sent).toHaveLength(2);
      expect(parseAttachmentMarkers(sent[1]).attachments.map(item => item.path)).toEqual([path]);
      expect(sent[1]).not.toContain("Please inspect");
      return;
    }
    expect(parseAttachmentMarkers(sent[0]).attachments.map((item) => item.path)).toEqual([path]);
    expect(sent[0].indexOf(path)).toBeGreaterThan(sent[0].indexOf("inspect"));
    expect(sent[0].indexOf(path)).toBeLessThan(sent[0].indexOf("here"));
    const chip = host.querySelector<HTMLButtonElement>('[aria-label="Sent message"] button[aria-label="Open image image1"]')!;
    expect(chip.querySelector("img")).toBeNull();
    await act(async () => chip.focus());
    expect(document.querySelector<HTMLImageElement>('[role="region"] img')?.src).toBe(pixels);
    expect(previews).toContain(path);
  } finally {
    await act(async () => root.unmount()); host.remove(); removeChatSend("picker", "sent");
    __setInvokeForTest(null); __setListenForTest(null); vi.unstubAllGlobals(); vi.restoreAllMocks();
  }
});


it("falls back to an image decoder when bitmap HEIF decoding fails and releases its URL", async () => {
  vi.stubGlobal("createImageBitmap", vi.fn(async () => { throw new Error("unsupported"); }));
  vi.stubGlobal("Image", class {
    naturalWidth = 2; naturalHeight = 2; onload: (() => void) | null = null;
    set src(_url: string) { this.onload?.(); }
  });
  const revoke = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
  vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:opaque");
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({ drawImage: vi.fn() } as unknown as CanvasRenderingContext2D);
  vi.spyOn(HTMLCanvasElement.prototype, "toBlob").mockImplementation(callback => callback(new Blob(["jpeg"], { type: "image/jpeg" })));
  try {
    const result = await prepareBrowserUpload(new Blob(["heif"], { type: "image/heif" }), "screen.heif");
    expect(result.name).toBe("screen.jpg");
    expect(result.blob.type).toBe("image/jpeg");
    expect(revoke).toHaveBeenCalledExactlyOnceWith("blob:opaque");
  } finally { vi.unstubAllGlobals(); vi.restoreAllMocks(); }
});
