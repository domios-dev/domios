import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { formatAttachmentMarker, type NormalizedChatMessage } from "@codeterm/chat-core";
import { LocalArtifactScope } from "./LocalArtifactScope";
import { MessageBubble } from "./MessageBubble";
import { configureLocalArtifactLoader, configureLocalArtifactOpener } from "./localArtifactLoader";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const SHOT = String.raw`C:\Users\dev\AppData\Local\Temp\codeterm-chat\a1-pasted-image-1.png`;
const SHOT_NAME = "a1-pasted-image-1.png";
const NOTES = "/tmp/codeterm-chat/c3-pasted-text-3.txt";
const PNG = "data:image/png;base64,iVBORw0KGgo=";
const mounted: { root: Root; host: HTMLElement }[] = [];

beforeEach(() => {
  configureLocalArtifactLoader(vi.fn(async (path: string) => {
    if (path !== SHOT) throw new Error("missing");
    return { data: PNG, media_type: "image/png" };
  }));
});

afterEach(async () => {
  for (const { root, host } of mounted.splice(0)) {
    await act(async () => root.unmount());
    host.remove();
  }
  configureLocalArtifactLoader(null);
  configureLocalArtifactOpener(null);
});

async function render(content: string): Promise<HTMLElement> {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  mounted.push({ root, host });
  const message: NormalizedChatMessage = { id: "u1", type: "user", content };
  await act(async () => root.render(<LocalArtifactScope tabId="owner"><MessageBubble message={message} /></LocalArtifactScope>));
  await act(async () => { await Promise.resolve(); });
  await act(async () => { await Promise.resolve(); });
  return host;
}

const capsules = (host: HTMLElement) => [...host.querySelectorAll<HTMLButtonElement>('[data-testid="chat-inline-attachment"]')];

describe("inline attachment capsules in the user bubble", () => {
  it("renders each capsule where it was typed, with the surrounding text in order", async () => {
    const host = await render(
      `compare ${formatAttachmentMarker(SHOT, "image", "image1")} with ${formatAttachmentMarker(NOTES, "text", "text1 · 312 lines")} please`,
    );
    expect(host.textContent).toBe("compare image1 with text1 · 312 lines please");
    expect(host.textContent).not.toContain("<domios");
    expect(host.textContent).not.toContain(SHOT);
    expect(capsules(host).map((button) => button.getAttribute("aria-label"))).toEqual([
      "Open image image1",
      "Reveal pasted text text1 · 312 lines",
    ]);
    expect(host.querySelector('[data-testid="chat-attachment-strip"]')).toBeNull();
  });

  it("opens an image capsule fullscreen on click", async () => {
    const host = await render(`see ${formatAttachmentMarker(SHOT, "image", "image1")}`);
    const [image] = capsules(host);
    expect(image.disabled).toBe(false);
    await act(async () => image.click());
    expect(document.querySelector(`[role="dialog"][aria-label="Image ${SHOT_NAME}"]`)).toBeTruthy();
  });

  it("reveals a file capsule through the host opener", async () => {
    const opener = vi.fn(async () => undefined);
    configureLocalArtifactOpener(opener);
    const host = await render(`read ${formatAttachmentMarker("/srv/repo", "folder", "repo")}`);
    const [folder] = capsules(host);
    expect(folder.getAttribute("aria-label")).toBe("Reveal folder repo");
    await act(async () => folder.click());
    expect(opener).toHaveBeenCalledWith("/srv/repo");
  });

  it("keeps chip-button attachments in the strip beside inline capsules", async () => {
    const host = await render(`${formatAttachmentMarker(SHOT, "image", "image1")} ok ${formatAttachmentMarker("/tmp/report.pdf")}`);
    expect(capsules(host)).toHaveLength(1);
    expect(host.querySelector('[data-testid="chat-attachment-strip"] [data-testid="chat-file-attachment"]')).toBeTruthy();
  });

  it("leaves a message without capsules exactly as before", async () => {
    const host = await render("plain words, no capsules");
    expect(capsules(host)).toHaveLength(0);
    expect(host.textContent).toBe("plain words, no capsules");
  });
});
