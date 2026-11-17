import { LocalArtifactScope } from "./LocalArtifactScope";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { MessageBubble } from "./MessageBubble";
import { formatAttachmentMarker, attachmentMarkerSegments, type MessageSegment } from "@codeterm/chat-core";
import { configureLocalArtifactLoader, configureLocalArtifactOpener } from "./localArtifactLoader";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const PATH = String.raw`C:\Users\dev\AppData\Local\Temp\codeterm-chat\e8a97274-pasted-image-1791277476989.png`;
const NAME = "e8a97274-pasted-image-1791277476989.png";
const PNG = "data:image/png;base64,iVBORw0KGgo=";

const mounted: { root: Root; host: HTMLElement }[] = [];
let loader: ReturnType<typeof vi.fn>;

beforeEach(() => {
  loader = vi.fn(async (path: string) => {
    if (path !== PATH) throw new Error("Unable to resolve Local preview file");
    return { data: PNG, media_type: "image/png" };
  });
  configureLocalArtifactLoader(loader);
});

afterEach(async () => {
  for (const { root, host } of mounted.splice(0)) {
    await act(async () => root.unmount());
    host.remove();
  }
  configureLocalArtifactLoader(null);
  configureLocalArtifactOpener(null);
});

async function flush() {
  await act(async () => { await Promise.resolve(); });
  await act(async () => { await Promise.resolve(); });
}

async function render(message: NormalizedChatMessage): Promise<HTMLElement> {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  mounted.push({ root, host });
  await act(async () => root.render(<LocalArtifactScope tabId="attachment-owner"><MessageBubble message={message} /></LocalArtifactScope>));
  await flush();
  return host;
}

const codexImageTurn = (content: string): NormalizedChatMessage => ({
  id: "codex-user-1",
  type: "user",
  content,
  attachments: [{ id: "codex-user-1-img-0", mimeType: "image/png", path: PATH }],
});

const openButton = (scope: ParentNode) =>
  [...scope.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.getAttribute("aria-label") === `Open image ${NAME}`);
const viewer = () => document.querySelector<HTMLElement>(`[role="dialog"][aria-label="Image ${NAME}"]`);

describe("user image attachments", () => {
  it("renders an image chip and reads its focus preview", async () => {
    const host = await render(codexImageTurn("[1 image]"));
    const button = openButton(host);
    expect(button).toBeTruthy();
    expect(button!.disabled).toBe(false);
    expect(button!.querySelector("img")).toBeNull();
    await act(async () => button!.focus());
    expect(document.querySelector('[role="region"] img')?.getAttribute("src")).toBe(PNG);
    expect(loader).toHaveBeenCalledWith(PATH, "image", "attachment-owner");
    expect(host.textContent).not.toContain("[1 image]");
    expect(host.textContent).not.toContain("<image");
  });

  it("touch opens a dismissible preview and Expand opens the shared viewer", async () => {
    const original = window.matchMedia;
    window.matchMedia = (query) => ({ matches: query === "(hover: none)", media: query, onchange: null, addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {}, dispatchEvent: () => true });
    try {
      const host = await render(codexImageTurn("[1 image]"));
      const trigger = openButton(host)!;
      await act(async () => trigger.dispatchEvent(new MouseEvent("click", { detail: 1, bubbles: true })));
      expect(viewer()).toBeNull();
      const preview = document.querySelector('[role="region"]')!;
      expect(preview).not.toBeNull();
      await act(async () => {
        trigger.dispatchEvent(new PointerEvent("pointerout", { bubbles: true, pointerType: "touch" }));
        preview.dispatchEvent(new PointerEvent("pointerout", { bubbles: true, pointerType: "touch" }));
      });
      expect(document.querySelector('[role="region"]')).not.toBeNull();
      await act(async () => preview.querySelector<HTMLButtonElement>(`[aria-label="Close preview ${NAME}"]`)!.click());
      expect(document.querySelector('[role="region"]')).toBeNull();
      await act(async () => trigger.dispatchEvent(new MouseEvent("click", { detail: 1, bubbles: true })));
      await act(async () => document.querySelector<HTMLButtonElement>(`[aria-label="Expand image ${NAME}"]`)!.click());
      expect(viewer()?.parentElement).toBe(document.body);
      await act(async () => document.querySelector<HTMLButtonElement>('[aria-label="Close image"]')!.click());
      expect(viewer()).toBeNull();
      expect(document.activeElement).toBe(trigger);
      expect(document.querySelector('[role="region"]')).toBeNull();
    } finally { window.matchMedia = original; }
  });

  it("keeps the caption beside the thumbnail", async () => {
    const host = await render(codexImageTurn("what is wrong here"));
    expect(host.textContent).toContain("what is wrong here");
    expect(openButton(host)).toBeTruthy();
  });

  it("opens the enlarged viewer on click and closes it on Escape", async () => {
    const host = await render(codexImageTurn("[1 image]"));
    await act(async () => openButton(host)!.click());
    const dialog = viewer();
    expect(dialog).toBeTruthy();
    expect(dialog!.querySelector(`img[alt="${NAME}"]`)?.getAttribute("src")).toBe(PNG);

    await act(async () => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", code: "Escape", bubbles: true }));
    });
    expect(viewer()).toBeNull();
  });

  it("is a focusable native button and focus returns after closing", async () => {
    const host = await render(codexImageTurn("[1 image]"));
    const button = openButton(host)!;
    expect(button.tagName).toBe("BUTTON");
    expect(button.type).toBe("button");
    await act(async () => button.focus());
    expect(document.activeElement).toBe(button);
    await act(async () => button.click());
    expect(viewer()!.contains(document.activeElement)).toBe(true);
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", code: "Escape", bubbles: true }));
    });
    expect(document.activeElement).toBe(button);
  });

  it("keeps an unreadable image as a compact capsule with retry", async () => {
    const host = await render({
      ...codexImageTurn("[1 image]"),
      attachments: [{ id: "gone", mimeType: "image/png", path: String.raw`C:\gone\missing.png` }],
    });
    const button = [...host.querySelectorAll<HTMLButtonElement>("button")].find(
      (candidate) => candidate.getAttribute("aria-label") === "Open image missing.png",
    );
    expect(button?.disabled).toBe(false);
    await act(async () => button!.focus());
    expect(document.querySelector('[role="region"]')).toBeNull();
    expect(button?.getAttribute("aria-label")).toBe("Open image missing.png");
    const attempts = loader.mock.calls.length;
    await act(async () => button!.click());
    await flush();
    expect(loader.mock.calls.length).toBeGreaterThan(attempts);
    expect(document.querySelector('[role="region"]')).toBeNull();
  });
});

const userTurn = (content: string): NormalizedChatMessage => ({ id: "user-1", type: "user", content });
const DOC = "/home/me/reports/q3 summary.pdf";

describe("attachment markers in the user bubble", () => {
  it("renders marker attachments as inline controls", async () => {
    const host = await render(userTurn(`check these ${formatAttachmentMarker(PATH)} ${formatAttachmentMarker(DOC)}`));
    expect(openButton(host)).toBeTruthy();
    expect(host.querySelector<HTMLButtonElement>('button[aria-label="Preview file q3 summary.pdf"]')?.disabled).toBe(false);
    expect(host.textContent).toContain("check these");
    expect(host.textContent).not.toContain("<domios");
    expect(host.textContent).not.toContain(PATH);
  });

  it("opens a marker image in the shared fullscreen overlay and reveals files through the host opener", async () => {
    const revealed: string[] = [];
    configureLocalArtifactOpener(async (path) => { revealed.push(path); });
    const host = await render(userTurn(`${formatAttachmentMarker(PATH)} ${formatAttachmentMarker(DOC)}`));
    await act(async () => openButton(host)!.click());
    expect(viewer()?.querySelector(`img[alt="${NAME}"]`)?.getAttribute("src")).toBe(PNG);
    await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="Reveal file q3 summary.pdf"]')!.click());
    expect(revealed).toEqual([DOC]);
  });

  it("renders an older message with raw temp paths exactly as before", async () => {
    const legacy = `look at this ${PATH}`;
    const host = await render(userTurn(legacy));
    expect(host.querySelector('[aria-label="Attachments"]')).toBeNull();
    expect(openButton(host)).toBeUndefined();
    expect(host.textContent).toContain(legacy);
  });
});

it("opens a labeled marker image through the named thumbnail control", async () => {
  const host = await render(userTurn(`${formatAttachmentMarker(PATH, "image", "image1")}caption`));
  const button = host.querySelector<HTMLButtonElement>(`button[aria-label="Open image ${NAME}"]`)!;
  expect(button.disabled).toBe(false);
  await act(async () => button.click());
  expect(viewer()?.querySelector(`img[alt="${NAME}"]`)?.getAttribute("src")).toBe(PNG);
});

it("retries a failed image read through the same scoped loader", async () => {
  loader.mockRejectedValueOnce(new Error("read failed"));
  const host = await render(codexImageTurn("caption"));
  await act(async () => openButton(host)!.focus());
  expect(document.querySelector('[role="region"]')).toBeNull();
  await act(async () => openButton(host)!.click());
  await flush();
  expect(loader).toHaveBeenLastCalledWith(PATH, "image", "attachment-owner");
  expect(document.querySelector('[role="region"]')).toBeNull();
  await act(async () => openButton(host)!.blur());
  await act(async () => openButton(host)!.focus());
  expect(document.querySelector('[role="region"] img')?.getAttribute("src")).toBe(PNG);
  await act(async () => openButton(host)!.click());
  expect(viewer()?.querySelector("img")?.getAttribute("src")).toBe(PNG);
});

it("collapses a browser image decode failure to the named capsule", async () => {
  const host = await render({
    ...codexImageTurn("caption"),
    attachments: [{ id: "inline", mimeType: "image/png", data: "AAAA", path: PATH }],
  });
  const chip = openButton(host)!;
  await act(async () => chip.focus());
  const image = document.querySelector<HTMLImageElement>('[role="region"] img')!;
  await act(async () => image.dispatchEvent(new Event("error")));
  expect(document.querySelector('[role="region"]')).toBeNull();
  expect(chip.isConnected).toBe(true);
  expect(chip.getAttribute("aria-label")).toBe(`Open image ${NAME}`);
});

it.each(["pointer", "keyboard"])("remembers a fullscreen image decode failure after %s activation", async activation => {
  const host = await render({
    ...codexImageTurn("caption"),
    attachments: [{ id: "inline", mimeType: "image/png", data: "AAAA", path: PATH }],
  });
  const chip = openButton(host)!;
  if (activation === "keyboard") await act(async () => chip.focus());
  await act(async () => chip.dispatchEvent(new MouseEvent("click", { detail: activation === "pointer" ? 1 : 0, bubbles: true })));
  expect(viewer()).toBeTruthy();
  const image = viewer()!.querySelector<HTMLImageElement>("img")!;
  await act(async () => image.dispatchEvent(new Event("error")));
  expect(viewer()).toBeNull();
  expect(chip.isConnected).toBe(true);
  expect(chip.getAttribute("aria-label")).toBe(`Open image ${NAME}`);
  await act(async () => chip.dispatchEvent(new MouseEvent("click", { bubbles: true })));
  expect(viewer()).toBeNull();
  expect(document.querySelector('[role="region"]')).toBeNull();
});

it.each([1280, 390])("shows the image preview on hover at viewport width %s", async (width) => {
  const previous = window.innerWidth;
  Object.defineProperty(window, "innerWidth", { configurable: true, value: width });
  try {
    const host = await render(codexImageTurn("caption"));
    const chip = openButton(host)!;
    await act(async () => chip.dispatchEvent(new PointerEvent("pointerover", { bubbles: true, pointerType: "mouse" })));
    expect(document.querySelector('[role="region"] img')?.getAttribute("src")).toBe(PNG);
    await act(async () => chip.click());
    expect(viewer()?.querySelector("img")?.getAttribute("src")).toBe(PNG);
  } finally {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: previous });
  }
});

it.each(["heic", "jpg", "png"])("shows a two-attachment %s photo with its real name and internal model preview", async extension => {
  const original = `C:/chat/${"a".repeat(32)}.${extension}`;
  const model = "C:/pad/inbox/opaque-model.jpg";
  const name = `IMG_4321.${extension}`;
  const content = `Look\n${formatAttachmentMarker(original, "file", "original")}\n\n\n${formatAttachmentMarker(model, "image", `${"a".repeat(28)}.heic`)}`;
  loader.mockImplementation(async path => {
    if (path !== model) throw new Error("Only the internal JPEG may be previewed");
    return { data: PNG, media_type: "image/jpeg" };
  });
  for (const structured of [false, true]) {
    const segments: MessageSegment[] = attachmentMarkerSegments(content).map(segment => "text" in segment ? { type: "text", text: segment.text }
      : { type: "attachment", kind: segment.attachment.kind, path: segment.attachment.path, label: segment.attachment.label ?? null, body: null, status: null });
    const host = await render({ id: `photo-${extension}-${structured}`, type: "user", content, attachmentNames: { [original]: name }, attachments: [{ id: "model-preview", mimeType: "image/jpeg", path: model }], ...(structured ? { segments } : {}) });
    const chips = host.querySelectorAll<HTMLButtonElement>('[data-testid="chat-inline-attachment"]');
    expect(chips).toHaveLength(1);
    expect(chips[0].getAttribute("data-attachment-path")).toBe(original);
    expect(chips[0].getAttribute("aria-label")).toBe(`Open image ${name}`);
    expect(host.textContent).toContain(name);
    expect(host.textContent).not.toContain("original");
    expect(host.textContent).not.toContain("a".repeat(16));
    await act(async () => chips[0].focus());
    await flush();
    expect(document.querySelector<HTMLImageElement>(`[role="region"][aria-label="Preview ${name}"] img`)?.src).toBe(PNG);
    expect(loader.mock.calls.every(([path]) => path === model)).toBe(true);
    await act(async () => chips[0].blur());
  }
});

it("keeps multiple real attachments compact and hides only the model-copy item", async () => {
  const original = "C:/chat/id-Holiday.heic";
  const model = "C:/pad/inbox/opaque-model.jpg";
  const content = [formatAttachmentMarker(original, "file", "original"), formatAttachmentMarker(model, "image", "image1"),
    formatAttachmentMarker("C:/chat/clip.mov", "video", "clip.mov"), formatAttachmentMarker("C:/chat/document.pdf", "file", "document.pdf")].join("\n\n\n");
  const host = await render({ id: "mixed", type: "user", content, attachmentNames: { [original]: "Holiday.heic" } });
  expect(host.querySelectorAll('[data-testid="chat-inline-attachment"]')).toHaveLength(3);
  const inline = host.querySelector('[data-testid="chat-inline-attachment-text"]')!;
  expect([...inline.childNodes].filter(node => node.nodeType === Node.TEXT_NODE).some(node => /\n\s*\n/.test(node.textContent ?? ""))).toBe(false);
  expect(inline.textContent).toContain("Holiday.heic");
  expect(inline.textContent).toContain("clip.mov");
  expect(inline.textContent).toContain("document.pdf");
});

it("groups a photo's preview copy even beside a voice attachment", async () => {
  const original = "C:/chat/photo.heic", model = "C:/pad/inbox/photo-model.jpg";
  const segments: MessageSegment[] = [
    { type: "attachment", kind: "voice", path: "C:/chat/note.wav", label: null, body: "A voice note", status: null },
    { type: "attachment", kind: "file", path: original, label: "original", body: null, status: null },
    { type: "text", text: "\n\n" },
    { type: "attachment", kind: "image", path: model, label: "image1", body: null, status: null },
  ];
  const host = await render({ id: "voice-photo", type: "user", content: "", segments, attachmentNames: { [original]: "IMG_4321.heic" } });
  expect(host.querySelectorAll('[data-testid="chat-inline-attachment"]')).toHaveLength(1);
  expect(host.textContent).toContain("IMG_4321.heic");
  expect(host.textContent).toContain("A voice note");
});
