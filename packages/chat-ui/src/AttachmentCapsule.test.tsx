import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DOMIOS_TEXT_MIME, formatAttachmentMarker, type NormalizedChatMessage } from "@codeterm/chat-core";
import { LocalArtifactScope } from "./LocalArtifactScope";
import { MessageBubble } from "./MessageBubble";
import { configureLocalArtifactLoader, configureLocalArtifactOpener } from "./localArtifactLoader";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const SHOT = String.raw`C:\Users\dev\AppData\Local\Temp\codeterm-chat\a1-pasted-image-1.png`;
const NOTES = "/tmp/codeterm-chat/c3-pasted-text-3.txt";

it("keeps text and multiple attachments in marker order in one inline flow", async () => {
  const host = await render(`before ${formatAttachmentMarker(SHOT, "image")} between ${formatAttachmentMarker(NOTES, "file")} after`);
  const flow = inlineText(host);
  const [image, file] = capsules(host);
  expect(flow.textContent).toBe("before a1-pasted-image-1.png between c3-pasted-text-3.txt after");
  expect(image.compareDocumentPosition(file) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(flow.querySelector("pre, p, br")).toBeNull();
});

it("names file references, truncates their basename, previews an existing file and mutes a missing file", async () => {
  const path = "/tmp/private-neighbor-with-a-long-name.txt";
  configureLocalArtifactLoader(vi.fn(async (requested: string) => {
    if (requested !== path) throw new Error("missing");
    return { data: "Private preview sentinel", media_type: "text/plain" };
  }));
  const reveal = vi.fn(async () => {});
  configureLocalArtifactOpener(reveal);
  const host = await render(`${formatAttachmentMarker(path)} ${formatAttachmentMarker("/tmp/missing.txt")}`);
  const [file, missing] = capsules(host);
  expect(file.textContent?.length).toBe(24);
  expect(file.title).toBe(path);
  expect(file.getAttribute("aria-label")).toBe("private-neighbor-with-a-long-name.txt, file");
  expect(missing.getAttribute("aria-disabled")).toBe("true");
  await act(async () => file.click());
  expect(document.querySelector('[data-testid="capsule-preview-text"]')?.textContent).toContain("Private preview sentinel");
  expect(reveal).not.toHaveBeenCalled();
});

it("keeps unpreviewable file types available through the existing file opener", async () => {
  const reveal = vi.fn(async () => {});
  configureLocalArtifactOpener(reveal);
  const host = await render(formatAttachmentMarker("/tmp/report.pdf"));
  const [file] = capsules(host);
  expect(file.getAttribute("aria-disabled")).toBeNull();
  await act(async () => file.click());
  expect(reveal).toHaveBeenCalledWith("/tmp/report.pdf");
});
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

function select(start: Node, startOffset: number, end: Node, endOffset: number) {
  const range = document.createRange();
  range.setStart(start, startOffset);
  range.setEnd(end, endOffset);
  window.getSelection()!.removeAllRanges();
  window.getSelection()!.addRange(range);
}

async function copy(target: Element) {
  const written: Record<string, string> = {};
  const event = Object.assign(new Event("copy", { bubbles: true, cancelable: true }), {
    clipboardData: { setData: (format: string, value: string) => { written[format] = value; } },
  });
  await act(async () => { target.dispatchEvent(event); });
  window.getSelection()!.removeAllRanges();
  return { written, prevented: event.defaultPrevented };
}

const inlineText = (host: HTMLElement) => host.querySelector<HTMLElement>('[data-testid="chat-inline-attachment-text"]')!;
function textNode(element: Element, fragment: string): Text {
  const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (node.nodeValue?.includes(fragment)) return node as Text;
  }
  throw new Error(`Missing descendant text: ${fragment}`);
}

const capsules = (host: HTMLElement) => [...host.querySelectorAll<HTMLButtonElement>('[data-testid="chat-inline-attachment"]')];

describe("inline attachment capsules in the user bubble", () => {
  it("names an uploaded image by the user filename through fullscreen without changing its path", async () => {
    const path = "/tmp/df9d0d2065b54cf28612788a0bbf750f-joy-chip.png";
    const loader = vi.fn(async (_path: string) => ({ data: PNG, media_type: "image/png" }));
    configureLocalArtifactLoader(loader);
    const host = await render(formatAttachmentMarker(path, "image", "image1"));
    const [button] = capsules(host);
    expect(button.getAttribute("aria-label")).toBe("Open image joy-chip.png");
    expect(button.querySelector('[aria-label="Image: joy-chip.png"]')).not.toBeNull();
    expect(button.getAttribute("data-attachment-path")).toBe(path);
    expect(loader.mock.calls[0]?.[0]).toBe(path);
    await act(async () => button.click());
    expect(document.querySelector('[role="dialog"][aria-label="Image joy-chip.png"]')).not.toBeNull();
  });

  it.each(["file", "image", "video"] as const)("names a %s attachment visibly and accessibly", async (kind) => {
    const name = kind === "file" ? "report.pdf" : kind === "image" ? "holiday.png" : "recording.mp4";
    const path = `/tmp/${name}`;
    const host = await render(formatAttachmentMarker(path, kind, `${name} · 2.3 MB`));
    const [button] = capsules(host);
    expect(button.textContent).toBe(kind === "image" ? name : `${name} · 2.3 MB`);
    expect(button.getAttribute("aria-label")).toContain(name);
    if (kind === "image") expect(button.getAttribute("aria-label")).toBe(`Open image ${name}`);
    expect(button.title).toBe(path);
  });

  it.each([["image", "image1", "a.bin"], ["video", "video2", "video"], ["quote", "quote3 · a line", "quote · a line"]] as const)("never shows the generated number of a %s capsule", async (kind, label, shown) => {
    const host = await render(formatAttachmentMarker("/tmp/a.bin", kind, label));
    const [button] = capsules(host);
    expect(button.textContent).toBe(shown);
    expect(button.getAttribute("data-attachment-label")).toBe(kind === "image" ? shown : label);
  });

  it.each([["image", "Open image", "png"], ["video", "Play video", "mp4"]] as const)("names a generated %s capsule by its file, never by its number", async (kind, verb, ext) => {
    const host = await render(formatAttachmentMarker(`/tmp/codeterm-chat/holiday.${ext}`, kind, `${kind}1`));
    const [button] = capsules(host);
    expect(button.getAttribute("aria-label")).toBe(`${verb} holiday.${ext}`);
  });

  it("renders each capsule where it was typed, with the surrounding text in order", async () => {
    const host = await render(
      `compare ${formatAttachmentMarker(NOTES, "text", "text1")} with ${formatAttachmentMarker(NOTES, "text", "text1 · 312 lines")} please`,
    );
    expect(host.textContent).toBe("compare text with text · 312 lines please");
    expect(host.textContent).not.toContain("<domios");
    expect(host.textContent).not.toContain(SHOT);
    expect(capsules(host).map((button) => button.getAttribute("aria-label"))).toEqual([
      "Preview pasted text c3-pasted-text-3.txt",
      "Preview pasted text c3-pasted-text-3.txt",
    ]);
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

  it("renders unlabeled file markers as inline capsules", async () => {
    const host = await render(`${formatAttachmentMarker(NOTES, "text", "text1")} ok ${formatAttachmentMarker("/tmp/report.pdf")}`);
    expect(capsules(host)).toHaveLength(2);
  });

  it("leaves a message without capsules exactly as before", async () => {
    const host = await render("plain words, no capsules");
    expect(capsules(host)).toHaveLength(0);
    expect(host.textContent).toBe("plain words, no capsules");
  });

  it("copies a selection as inline labels for other apps and as attachment tags for a composer", async () => {
    const context = formatAttachmentMarker(NOTES, "text", "text1");
    const notes = formatAttachmentMarker(NOTES, "text", "text1 · 312 lines");
    const host = await render(`compare ${context} with ${notes} please`);
    const text = inlineText(host);
    select(textNode(text, "compare"), 2, textNode(text, "please"), 5);
    const { written, prevented } = await copy(text);
    expect(prevented).toBe(true);
    expect(written["text/plain"]).toBe("mpare text with text · 312 lines plea");
    expect(written[DOMIOS_TEXT_MIME]).toBe(`mpare ${context} with ${notes} plea`);
  });

  it("copies a capsule whole when the selection starts inside its label", async () => {
    const context = formatAttachmentMarker(NOTES, "text", "text1");
    const host = await render(`see ${context} now`);
    const [capsule] = capsules(host);
    const label = [...capsule.querySelectorAll("span")].flatMap((span) => [...span.childNodes]).find((node) => node.nodeValue === "text")!;
    const text = inlineText(host);
    select(label, 3, textNode(text, "now"), 4);
    const { written } = await copy(text);
    expect(written["text/plain"]).toBe("text now");
    expect(written[DOMIOS_TEXT_MIME]).toBe(`${context} now`);
  });

  it("leaves a copy without capsules to the browser", async () => {
    const host = await render(`plain start ${formatAttachmentMarker(NOTES, "text", "text1")}`);
    const text = inlineText(host);
    select(textNode(text, "plain"), 0, textNode(text, "plain"), 5);
    const { written, prevented } = await copy(text);
    expect(prevented).toBe(false);
    expect(written).toEqual({});
  });
});

describe("context capsules", () => {
  it("preview the head of their stored text on focus and name the capsule, not its temp file", async () => {
    const QUOTE = "/tmp/codeterm-chat/q1-quote-notes.md";
    configureLocalArtifactLoader(vi.fn(async (path: string) => {
      if (path !== QUOTE) throw new Error("missing");
      return { data: "From `/d/notes.md:3`:\n> First paragraph here.", media_type: "text/markdown" };
    }));
    configureLocalArtifactOpener(async () => {});
    const host = await render(`see ${formatAttachmentMarker(QUOTE, "quote", "quote1 · notes.md:3")}`);
    const [capsule] = capsules(host);
    expect(capsule.dataset.attachmentKind).toBe("quote");
    await act(async () => capsule.focus());
    await act(async () => { await Promise.resolve(); });
    expect(document.querySelector('[data-testid="capsule-preview-text"]')?.textContent).toBe("From `/d/notes.md:3`:\n> First paragraph here.");
    expect(document.querySelector('[role="region"]')).toBeTruthy();
    expect(host.textContent).not.toContain("q1-quote-notes.md");
  });
});
