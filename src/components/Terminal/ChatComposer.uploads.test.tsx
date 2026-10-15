import { ComposerIssuesProvider } from "./ComposerIssuesContext";
import { AgentRuntimeBanner } from "./AgentRuntimeBanner";
import { beforeEach as beforeDaemonCase } from "vitest";
import { configureDaemonFixture } from "../../test/daemonTransport.testdata";
beforeDaemonCase(() => configureDaemonFixture());
import { Blob as BrowserBlob, File as BrowserFile, FormData as BrowserFormData } from "happy-dom";
import { act, useState } from "react";
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

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const path = "/home/owner/.codeterm-dev/uploads/id-photo.png";
const pixels = "data:image/png;base64,iVBORw0KGgo=";

it.each(["success", "HTTP 503", "NotReadableError"])("web picker retains and sends its image after editing text: %s", async (failure) => {
  vi.stubGlobal("Blob", BrowserBlob);
  vi.stubGlobal("File", BrowserFile);
  vi.stubGlobal("FormData", BrowserFormData);
  const sent: string[] = [];
  const previews: string[] = [];
  let requests = 0;
  const fetch = vi.fn(async (_url: unknown, options?: RequestInit) => {
    expect(String(_url)).toContain("/api/upload");
    const body = options?.body as FormData;
    const file = body.get("file") as File;
    expect(file.type).toBe("image/png");
    expect(file.size).toBe(5);
    if (++requests === 1 && failure === "HTTP 503") return new Response("busy", { status: 503 });
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
    return <MuxStateStoreContext.Provider value={store}><LocalArtifactScope tabId="picker">
      {bubble && <div role="region" aria-label="Sent message"><MessageBubble message={{ id: "sent", type: "user", content: bubble }} /></div>}
      <ComposerIssuesProvider><AgentRuntimeBanner state={null} onRetry={() => {}} /><ChatComposer tabId="picker" tabProvider={null} currentPtyId={null} pendingPrompt={null}
        attachments={attachments} addAttachments={(paths) => setAttachments((current) => [...current, ...paths])}
        removeAttachment={(path) => setAttachments((current) => current.filter((item) => item !== path))}
        clearAttachments={() => setAttachments([])} handlePasteEvent={() => {}} onWillSendMessage={() => {}}
        addOptimistic={(content) => { setBubble(content); return "sent"; }} updateOptimistic={(_id, content) => setBubble(content)}
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
    const file = new File(["image"], "photo.png", { type: "image/png" });
    if (failure === "NotReadableError") {
      const read = file.arrayBuffer.bind(file);
      let attempts = 0;
      file.arrayBuffer = () => ++attempts === 1
        ? Promise.reject(new DOMException("The I/O read operation failed.", "NotReadableError")) : read();
    }
    Object.defineProperty(input, "files", { value: [file] });
    await act(async () => input.dispatchEvent(new Event("change", { bubbles: true })));
    expect(fetch.mock.calls.length, host.textContent ?? "").toBe(failure === "NotReadableError" ? 0 : 1);
    if (failure !== "success") {
      await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="1 attachment failed"]')!.click());
      const retry = host.querySelector<HTMLButtonElement>('[aria-label="Retry attachment photo.png"]');
      expect(retry, host.textContent ?? "").not.toBeNull();
      await act(async () => retry!.click());
    }
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(editor, `${editor.value} after upload`);
      editor.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(capsuleSpans(editor.value)).toHaveLength(1);
    await act(async () => editor.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", code: "Enter", bubbles: true })));
    expect(sent).toHaveLength(1);
    expect(parseAttachmentMarkers(sent[0]).attachments.map((item) => item.path)).toEqual([path]);
    expect(sent[0].indexOf(path)).toBeGreaterThan(sent[0].indexOf("inspect"));
    expect(sent[0].indexOf(path)).toBeLessThan(sent[0].indexOf("here"));
    const chip = host.querySelector<HTMLButtonElement>('[aria-label="Sent message"] button[aria-label="Open image image1"]')!;
    expect(chip.querySelector("img")).toBeNull();
    await act(async () => chip.focus());
    expect(document.querySelector<HTMLImageElement>('[role="region"] img')?.src).toBe(pixels);
    expect(previews).toContain(path);
  } finally {
    await act(async () => root.unmount()); host.remove();
    __setInvokeForTest(null); __setListenForTest(null); vi.unstubAllGlobals();
  }
});
