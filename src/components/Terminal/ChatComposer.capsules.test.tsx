import { act, createRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { DOMIOS_TEXT_MIME, formatAttachmentMarker, type NormalizedChatMessage } from "@codeterm/chat-core";
import { LocalArtifactScope, MessageBubble } from "@codeterm/chat-ui";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { ChatComposer } from "./ChatComposer";
import { capsuleToken } from "./composerCapsules";
import type { InlineAttachmentSink } from "./useComposerCapsules";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const SHOT_1 = String.raw`C:\Users\dev\AppData\Local\Temp\codeterm-chat\a1-pasted-image-1.png`;
const SHOT_2 = String.raw`C:\Users\dev\AppData\Local\Temp\codeterm-chat\b2-pasted-image-2.png`;
const stored = vi.hoisted(() => ({
  path: String.raw`C:\Users\dev\AppData\Local\Temp\codeterm-chat\c3-pasted-text.txt`,
  blobs: [] as { name: string; text: Promise<string> }[],
}));
const TEXT_FILE = stored.path;

vi.mock("./voice/audio", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./voice/audio")>()),
  storeBlob: async (blob: Blob, name: string) => {
    stored.blobs.push({ name, text: blob.text() });
    return stored.path;
  },
}));

const muxState = (): ExtendedState => ({
  workspaces: [], tabs: {}, teams: [], active_workspace_id: null, focused_tab_id: null, notifications: [],
  tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [],
});

interface Call { command: string; args: Record<string, unknown> }
let root: Root | null = null;
let host: HTMLDivElement | null = null;

afterEach(async () => {
  await act(async () => root?.unmount()); root = null; host?.remove(); host = null;
  __setInvokeForTest(null); __setListenForTest(null);
  try { window.localStorage.clear(); } catch { /* storage may be unavailable */ }
});

async function renderComposer(tabId: string) {
  const calls: Call[] = [];
  __setInvokeForTest((async (command: string, args: Record<string, unknown> = {}) => {
    calls.push({ command, args });
    if (command === "delayed_send_list") return [];
    if (command === "chat_send_text") return { send_id: "s", tab_id: tabId, state: "written", error: null, ledger_id: "l", sequence: 1, retained_from: 1 };
    return {};
  }) as never);
  __setListenForTest((async () => () => {}) as never);
  const sink = createRef<InlineAttachmentSink | null>() as { current: InlineAttachmentSink | null };
  const handlePasteEvent = vi.fn();
  host = document.createElement("div"); document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<MuxStateStoreContext.Provider value={createMuxStateStore(muxState)}>
    <ChatComposer tabId={tabId} tabProvider={null} currentPtyId={null} pendingPrompt={null} attachments={[]} addAttachments={() => {}}
      removeAttachment={() => {}} clearAttachments={() => {}} handlePasteEvent={handlePasteEvent} onWillSendMessage={() => {}}
      addOptimistic={() => "optimistic"} updateOptimistic={() => {}} observeChatSendStatus={() => {}} showComposerError={() => {}}
      inlineAttachmentSink={sink} />
  </MuxStateStoreContext.Provider>));
  const textarea = host.querySelector<HTMLTextAreaElement>('[data-testid="chat-composer"]')!;
  return { textarea, sink, calls, handlePasteEvent };
}

async function setValue(el: HTMLTextAreaElement, value: string, caret = value.length) {
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(el, value);
    el.setSelectionRange(caret, caret);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

async function key(el: HTMLTextAreaElement, code: string) {
  await act(async () => { el.dispatchEvent(new KeyboardEvent("keydown", { key: code, code, bubbles: true, cancelable: true })); });
}

async function paste(el: HTMLTextAreaElement, data: string | Record<string, string>) {
  const flavours = typeof data === "string" ? { "text/plain": data } : data;
  const event = Object.assign(new Event("paste", { bubbles: true, cancelable: true }), {
    clipboardData: { items: [], getData: (type: string) => flavours[type] ?? "" },
  });
  await act(async () => { el.dispatchEvent(event); });
  await act(async () => { await Promise.resolve(); });
  await act(async () => { await Promise.resolve(); });
  return event;
}

async function copyFrom(target: Element, type: "copy" | "cut" = "copy") {
  const written: Record<string, string> = {};
  const event = Object.assign(new Event(type, { bubbles: true, cancelable: true }), {
    clipboardData: { setData: (format: string, value: string) => { written[format] = value; } },
  });
  await act(async () => { target.dispatchEvent(event); });
  return { written, prevented: event.defaultPrevented };
}

async function sendAndRead(textarea: HTMLTextAreaElement, calls: Call[]) {
  await key(textarea, "Enter");
  await act(async () => { await Promise.resolve(); });
  return calls.filter((call) => call.command === "chat_send_text").map((call) => call.args.text);
}

const mirror = () => document.querySelector('[data-testid="composer-capsule-mirror"]');

it("inserts dropped images as numbered capsules at the caret and sends tags with absolute paths", async () => {
  const { textarea, sink, calls } = await renderComposer("capsule-insert");
  await setValue(textarea, "look  here", 5);
  await act(async () => sink.current?.([{ path: SHOT_1, kind: "image" }, { path: SHOT_2, kind: "image" }]));
  expect(textarea.value).toBe(`look ${capsuleToken("image1")} ${capsuleToken("image2")} here`);
  expect(mirror()?.textContent).toContain("image1");
  expect(await sendAndRead(textarea, calls)).toEqual([
    `look ${formatAttachmentMarker(SHOT_1, "image", "image1")} ${formatAttachmentMarker(SHOT_2, "image", "image2")} here`,
  ]);
  expect(textarea.value).toBe("");
});

it("removes a whole capsule with Backspace and brings it back when the text is restored", async () => {
  const { textarea, sink, calls } = await renderComposer("capsule-backspace");
  await setValue(textarea, "see ");
  await act(async () => sink.current?.([{ path: SHOT_1, kind: "image" }]));
  const withCapsule = textarea.value;
  textarea.setSelectionRange(withCapsule.trimEnd().length, withCapsule.trimEnd().length);
  await key(textarea, "Backspace");
  expect(textarea.value).toBe("see  ");
  expect(mirror()).toBeNull();
  await setValue(textarea, withCapsule);
  expect(mirror()?.textContent).toContain("image1");
  expect(await sendAndRead(textarea, calls)).toEqual([`see ${formatAttachmentMarker(SHOT_1, "image", "image1")}`]);
});

it("saves a long paste to a file and inserts a text capsule the agent reads by path", async () => {
  const { textarea, calls, handlePasteEvent } = await renderComposer("capsule-long-paste");
  const long = Array.from({ length: 50 }, (_, i) => `line ${i}`).join("\n");
  const event = await paste(textarea, long);
  expect(event.defaultPrevented).toBe(true);
  expect(handlePasteEvent).not.toHaveBeenCalled();
  const [saved] = stored.blobs.splice(0);
  expect(saved.name).toMatch(/^pasted-text-\d+\.txt$/);
  expect(await saved.text).toBe(long);
  expect(textarea.value).toBe(`${capsuleToken("text1 · 50 lines")} `);
  expect(await sendAndRead(textarea, calls)).toEqual([formatAttachmentMarker(TEXT_FILE, "text", "text1 · 50 lines")]);
});

it("keeps a short paste as ordinary text", async () => {
  const { textarea, handlePasteEvent } = await renderComposer("capsule-short-paste");
  const event = await paste(textarea, "just a few words");
  expect(event.defaultPrevented).toBe(false);
  expect(handlePasteEvent).toHaveBeenCalledTimes(1);
});

it("pasting copied tags restores capsules", async () => {
  const { textarea, calls } = await renderComposer("capsule-paste-tags");
  await paste(textarea, `again ${formatAttachmentMarker(SHOT_2, "image", "image1")}`);
  expect(textarea.value).toBe(`again ${capsuleToken("image1")}`);
  expect(await sendAndRead(textarea, calls)).toEqual([`again ${formatAttachmentMarker(SHOT_2, "image", "image1")}`]);
});

it("restores a draft with its capsules after the composer remounts", async () => {
  const first = await renderComposer("capsule-draft");
  await setValue(first.textarea, "keep ");
  await act(async () => first.sink.current?.([{ path: SHOT_1, kind: "image" }]));
  const draft = first.textarea.value;
  await act(async () => root?.unmount()); root = null; host?.remove(); host = null;

  const second = await renderComposer("capsule-draft");
  expect(second.textarea.value).toBe(draft);
  expect(mirror()?.textContent).toContain("image1");
  expect(await sendAndRead(second.textarea, second.calls)).toEqual([`keep ${formatAttachmentMarker(SHOT_1, "image", "image1")}`]);
});

it("leaves an old plain draft untouched", async () => {
  window.localStorage.setItem("codeterm:composer-draft:tab-session-chat:capsule-old", "an older draft");
  const { textarea, calls } = await renderComposer("capsule-old");
  expect(textarea.value).toBe("an older draft");
  expect(mirror()).toBeNull();
  expect(await sendAndRead(textarea, calls)).toEqual(["an older draft"]);
});

it("copying a composer selection with capsules writes their tags in the plain and Domios flavours", async () => {
  const { textarea, sink } = await renderComposer("capsule-copy");
  await setValue(textarea, "see ");
  await act(async () => sink.current?.([{ path: SHOT_1, kind: "image" }]));
  textarea.setSelectionRange(0, textarea.value.length);
  const tagged = `see ${formatAttachmentMarker(SHOT_1, "image", "image1")}`;
  const { written, prevented } = await copyFrom(textarea);
  expect(prevented).toBe(true);
  expect(written["text/plain"].trimEnd()).toBe(tagged);
  expect(written[DOMIOS_TEXT_MIME]).toBe(written["text/plain"]);
});

it("a selection copied from a sent bubble pastes back as inline capsules", async () => {
  const sent = `inspect ${formatAttachmentMarker("/tmp/first-selection.txt", "text", "text2")} and ${formatAttachmentMarker(TEXT_FILE, "text", "text1 · 702 lines")} please`;
  const bubbleHost = document.createElement("div");
  document.body.append(bubbleHost);
  const bubbleRoot = createRoot(bubbleHost);
  const message: NormalizedChatMessage = { id: "u1", type: "user", content: sent };
  await act(async () => bubbleRoot.render(<LocalArtifactScope tabId="owner"><MessageBubble message={message} /></LocalArtifactScope>));
  const text = bubbleHost.querySelector('[data-testid="chat-inline-attachment-text"]')!;
  const range = document.createRange();
  range.selectNodeContents(text);
  window.getSelection()!.removeAllRanges();
  window.getSelection()!.addRange(range);
  const { written } = await copyFrom(text);
  window.getSelection()!.removeAllRanges();
  await act(async () => bubbleRoot.unmount());
  bubbleHost.remove();
  expect(written["text/plain"]).toBe("inspect text2 and text1 · 702 lines please");

  const { textarea, calls } = await renderComposer("capsule-bubble-paste");
  await paste(textarea, written);
  expect(textarea.value).toBe(`inspect ${capsuleToken("text2")} and ${capsuleToken("text1 · 702 lines")} please`);
  expect(await sendAndRead(textarea, calls)).toEqual([sent]);
});

it("typing after a chip appends normally over an inert mirror", async () => {
  const { textarea, sink } = await renderComposer("capsule-typing");
  await setValue(textarea, "see ");
  await act(async () => sink.current?.([{ path: SHOT_1, kind: "image" }]));
  await setValue(textarea, `${textarea.value}вот тут`);
  expect(textarea.value).toBe(`see ${capsuleToken("image1")} вот тут`);
  expect(mirror()?.textContent).toBe(textarea.value);
  expect(mirror()?.getAttribute("aria-hidden")).not.toBeNull();
  expect(mirror()?.querySelectorAll("button, a, input, [tabindex]")).toHaveLength(0);
});
