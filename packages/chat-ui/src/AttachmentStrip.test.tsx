import { LocalArtifactScope } from "./LocalArtifactScope";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { MessageBubble } from "./MessageBubble";
import { formatAttachmentMarker } from "@codeterm/chat-core";
import { configureLocalArtifactLoader, configureLocalArtifactOpener } from "./localArtifactLoader";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const PATH = String.raw`C:\Users\User\AppData\Local\Temp\codeterm-chat\e8a97274-pasted-image-1791277476989.png`;
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

  it("keeps an unreadable image focusable with retry", async () => {
    const host = await render({
      ...codexImageTurn("[1 image]"),
      attachments: [{ id: "gone", mimeType: "image/png", path: String.raw`C:\gone\missing.png` }],
    });
    const button = [...host.querySelectorAll<HTMLButtonElement>("button")].find(
      (candidate) => candidate.getAttribute("aria-label") === "Open image missing.png",
    );
    expect(button?.disabled).toBe(false);
    await act(async () => button!.focus());
    expect(document.querySelector('[role="alert"]')).toBeTruthy();
    expect(document.querySelector('button[aria-label="Retry image missing.png"]')).toBeTruthy();
  });
});

const userTurn = (content: string): NormalizedChatMessage => ({ id: "user-1", type: "user", content });
const DOC = "/home/me/reports/q3 summary.pdf";

describe("attachment markers in the user bubble", () => {
  it("renders marker attachments as inline controls", async () => {
    const host = await render(userTurn(`check these ${formatAttachmentMarker(PATH)} ${formatAttachmentMarker(DOC)}`));
    expect(openButton(host)).toBeTruthy();
    expect(host.querySelector<HTMLButtonElement>('button[aria-label="Reveal file q3 summary.pdf"]')?.disabled).toBe(true);
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
  const button = host.querySelector<HTMLButtonElement>('button[aria-label="Open image image1"]')!;
  expect(button.disabled).toBe(false);
  await act(async () => button.click());
  expect(viewer()?.querySelector(`img[alt="${NAME}"]`)?.getAttribute("src")).toBe(PNG);
});

it("retries a failed image read through the same scoped loader", async () => {
  loader.mockRejectedValueOnce(new Error("read failed"));
  const host = await render(codexImageTurn("caption"));
  await act(async () => openButton(host)!.focus());
  const retry = document.querySelector<HTMLButtonElement>(`button[aria-label="Retry image ${NAME}"]`)!;
  expect(retry).toBeTruthy();
  await act(async () => retry.click());
  expect(loader).toHaveBeenLastCalledWith(PATH, "image", "attachment-owner");
  expect(document.querySelector('[role="region"] img')?.getAttribute("src")).toBe(PNG);
  await act(async () => openButton(host)!.click());
  expect(viewer()?.querySelector("img")?.getAttribute("src")).toBe(PNG);
});

it.each([1280, 390])("shows the image preview on hover at viewport width %s", async (width) => {
  const previous = window.innerWidth;
  Object.defineProperty(window, "innerWidth", { configurable: true, value: width });
  try {
    const host = await render(codexImageTurn("caption"));
    const chip = openButton(host)!;
    await act(async () => chip.dispatchEvent(new MouseEvent("mouseover", { bubbles: true })));
    expect(document.querySelector('[role="region"] img')?.getAttribute("src")).toBe(PNG);
    await act(async () => chip.click());
    expect(viewer()?.querySelector("img")?.getAttribute("src")).toBe(PNG);
  } finally {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: previous });
  }
});
