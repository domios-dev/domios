import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { AttachmentGallery } from "./attachments";
const window = new Window({ url: "http://localhost/gallery" });
for (const key of ["window", "document", "navigator", "HTMLElement", "Element", "Node"] as const) Object.defineProperty(globalThis, key, { value: key === "window" ? window : (window as unknown as Record<string, unknown>)[key], configurable: true });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
async function main() {
  const host = document.createElement("div");document.body.append(host);const root = createRoot(host);
  await act(async () => root.render(<AttachmentGallery planId="opaque" attachments={[{ id: "photo", name: "photo.png", path: "/opaque/photo.png", media_type: "image/png", size_bytes: 10, created_at: 0 }]} />));
  const opener = host.querySelector<HTMLButtonElement>('[aria-label="Open image photo.png"]')!;
  await act(async () => opener.click());
  const dialog = document.querySelector('[role="dialog"][aria-label="Image photo.png"]')!;
  assert.equal(dialog.parentElement, document.body);
  assert(dialog.querySelector('[aria-label="Close fullscreen viewer"]'));
  assert.equal(host.contains(dialog), false);
  await act(async () => window.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Escape", code: "Escape", cancelable: true })));
  assert.equal(document.querySelector('[role="dialog"]'), null);
  assert.equal(document.activeElement, opener);
  assert(host.querySelector('[data-testid="plan-attachment-gallery"]'));
  await act(async () => root.unmount());host.remove();console.log("attachment gallery: shared viewport viewer, Escape and focus restore passed");
}
void main();
