import assert from "node:assert/strict";
import { Window } from "happy-dom";
import React from "react";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { StreamPreviewFloat } from "./StreamPreviewFloat";

const window = new Window({ url: "http://localhost/" });
const host = window.document.createElement("div") as unknown as HTMLDivElement;
window.document.body.appendChild(host);
Object.assign(globalThis, {
  document: window.document,
  window,
  HTMLElement: window.HTMLElement,
  Element: window.Element,
  Node: window.Node,
  IS_REACT_ACT_ENVIRONMENT: true,
  requestAnimationFrame: (callback: FrameRequestCallback) => callback(0),
  cancelAnimationFrame: () => undefined,
});
(window as unknown as { matchMedia: (query: string) => MediaQueryList }).matchMedia = () => ({ matches: false }) as MediaQueryList;
const root = createRoot(host);

async function main() {
  await act(async () => {
    root.render(<StreamPreviewFloat rows={[]} />);
  });
  assert.equal(host.querySelector('[data-testid="chat-stream-preview"]'), null, "status alone opens no preview");

  await act(async () => {
    root.render(<StreamPreviewFloat rows={["first streamed line", "second streamed line"]} />);
  });
  const preview = host.querySelector<HTMLElement>('[data-testid="chat-stream-preview"]');
  assert(preview, "real streamed rows open the preview");
  assert.equal(
    preview.querySelector('[data-testid="chat-stream-preview-line"]')?.textContent?.trim(),
    "first streamed line\nsecond streamed line",
  );

  await act(async () => {
    root.render(<StreamPreviewFloat rows={[]} />);
  });
  const closing = host.querySelector<HTMLElement>('[data-testid="chat-stream-preview"]');
  assert(closing, "preview stays mounted while its exit transition runs");
  assert.equal(closing.style.height, "0px");
}

void main();
