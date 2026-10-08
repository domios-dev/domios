import { LocalArtifactScope } from "./LocalArtifactScope";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { MessageBubble } from "./MessageBubble";
import { formatAttachmentMarker } from "@codeterm/chat-core";
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
  it("shows a pasted image path attachment as a thumbnail without the host's image-count text", async () => {
    const host = await render(codexImageTurn("[1 image]"));
    const button = openButton(host);
    expect(button).toBeTruthy();
    expect(button!.disabled).toBe(false);
    expect(button!.querySelector("img")?.getAttribute("src")).toBe(PNG);
    expect(loader).toHaveBeenCalledWith(PATH, "image", "attachment-owner");
    expect(host.textContent).not.toContain("[1 image]");
    expect(host.textContent).not.toContain("<image");
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

  it("is a focusable native button so Enter activates it, and focus returns after closing", async () => {
    const host = await render(codexImageTurn("[1 image]"));
    const button = openButton(host)!;
    expect(button.tagName).toBe("BUTTON");
    expect(button.type).toBe("button");
    button.focus();
    expect(document.activeElement).toBe(button);
    await act(async () => button.click());
    expect(viewer()!.contains(document.activeElement)).toBe(true);
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", code: "Escape", bubbles: true }));
    });
    expect(document.activeElement).toBe(button);
  });

  it("leaves an unreadable image as a disabled named control", async () => {
    const host = await render({
      ...codexImageTurn("[1 image]"),
      attachments: [{ id: "gone", mimeType: "image/png", path: String.raw`C:\gone\missing.png` }],
    });
    const button = [...host.querySelectorAll<HTMLButtonElement>("button")].find(
      (candidate) => candidate.getAttribute("aria-label") === "Open image missing.png",
    );
    expect(button?.disabled).toBe(true);
  });
});

const userTurn = (content: string): NormalizedChatMessage => ({ id: "user-1", type: "user", content });
const DOC = "/home/me/reports/q3 summary.pdf";

describe("attachment markers in the user bubble", () => {
  it("renders marker attachments as a strip and keeps only the typed text", async () => {
    const host = await render(userTurn(`check these ${formatAttachmentMarker(PATH)} ${formatAttachmentMarker(DOC)}`));
    const strip = host.querySelector('[role="group"][aria-label="Attachments"]');
    expect(strip).toBeTruthy();
    expect(openButton(strip!)?.querySelector("img")?.getAttribute("src")).toBe(PNG);
    expect(loader).toHaveBeenCalledWith(PATH, "image", "attachment-owner");
    expect(strip!.querySelector<HTMLButtonElement>('button[aria-label="Reveal file q3 summary.pdf"]')?.disabled).toBe(true);
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
