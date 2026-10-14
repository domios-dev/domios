import { Window } from "happy-dom";
import React, { act } from "react";
import { createRoot } from "react-dom/client";

import { formatAttachmentMarker, type NormalizedChatMessage } from "@codeterm/chat-core";
import { MessageBubble } from "./MessageBubble";

const windowInstance = new Window({ url: "http://localhost/chat-ui-notice-tests" });
const testWindow = windowInstance as unknown as Window & typeof globalThis;

(globalThis as { window: Window & typeof globalThis }).window = testWindow;
(globalThis as { document: Document }).document = testWindow.document;
Object.defineProperty(globalThis, "navigator", { configurable: true, value: testWindow.navigator });
(globalThis as { HTMLElement: typeof HTMLElement }).HTMLElement = testWindow.HTMLElement;
(globalThis as { HTMLButtonElement: typeof HTMLButtonElement }).HTMLButtonElement = testWindow.HTMLButtonElement;
(globalThis as { Node: typeof Node }).Node = testWindow.Node;
(globalThis as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

const snip = [
  "## Browser snip from the user",
  formatAttachmentMarker("/tmp/snips/snip-s1.png", "image"),
  "**Page:** developer.apple.com — `https://developer.apple.com/`",
  "**User says:** what is inside?",
].join("\n");

async function run() {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const systemNotice: NormalizedChatMessage = { id: "snip-notice", type: "user", content: snip, inboundKind: "system" };
  const fileNotice: NormalizedChatMessage = {
    id: "file-notice",
    type: "user",
    content: `Logs attached\n${formatAttachmentMarker("/tmp/run.log", "file")}`,
    inboundKind: "system",
  };

  await act(async () => {
    root.render(
      <div>
        <MessageBubble message={systemNotice} origin={{ kind: "system", context: "surface-annotation" }} />
        <MessageBubble message={fileNotice} origin={{ kind: "system", context: "lifecycle" }} />
      </div>,
    );
  });

  const [snipCard, fileCard] = Array.from(host.querySelectorAll<HTMLElement>('[data-testid="chat-system-event"]'));
  await act(async () => {
    snipCard?.querySelector<HTMLButtonElement>("button")?.click();
    fileCard?.querySelector<HTMLButtonElement>("button")?.click();
  });

  assert(snipCard?.querySelector('[data-testid="chat-inline-attachment"][data-attachment-kind="image"]'), "a notice's image marker renders as an image control");
  assert(!snipCard.textContent?.includes("<domios"), "the attachment marker never shows as raw text");
  const bold = Array.from(snipCard.querySelectorAll("strong")).map((node) => node.textContent);
  assert(bold.includes("Page:") && bold.includes("User says:"), "markdown bold renders after the marker line");
  assert(!snipCard.textContent?.includes("**Page:**"), "no raw markdown asterisks remain");
  assert(snipCard.querySelector("code")?.textContent === "https://developer.apple.com/", "inline code renders");
  assert(fileCard?.querySelector('[data-testid="chat-inline-attachment"][data-attachment-kind="file"]'), "a notice's file marker renders as a file control");

  await act(async () => root.unmount());
  host.remove();
  console.log("ok - notice cards render markdown and attachments");
}

void run();
