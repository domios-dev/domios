// Run: npx tsx src/components/Terminal/DelayedSendPanel.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { DelayedSendQueuePanel } from "./DelayedSendPanel";
import type { DelayedMessage } from "./delayedSend";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
  Object.defineProperty(globalThis, "PointerEvent", {
    value: window.PointerEvent ?? window.MouseEvent,
    configurable: true,
  });
  Object.defineProperty(globalThis, "requestAnimationFrame", {
    value: (callback: FrameRequestCallback) => {
      callback(0);
      return 0;
    },
    configurable: true,
  });
}

async function render(ui: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(ui));
  return { host, root };
}

setupDom();

async function main() {
  const originalText = [
    "A result before code:",
    "",
    "Code",
    "json",
    "·",
    "1 lines",
    "",
    "Copy",
    "",
    "Fullscreen",
    '{"ready":true}',
    "Message after code.",
  ].join("\n");
  const message: DelayedMessage = {
    id: "queued-1",
    tabId: "tab-1",
    text: originalText,
    scheduledAtMs: Date.now() + 20 * 60_000,
    createdAtMs: Date.now(),
    state: "queued",
    sessionChanged: false,
  };
  let sentId = "";
  const { host, root } = await render(
    <DelayedSendQueuePanel
      messages={[message]}
      busyId={null}
      touchMode={false}
      onClose={() => undefined}
      onDelete={() => undefined}
      onSendNow={(id) => { sentId = id; }}
      onUpdate={() => undefined}
    />,
  );

  const preview = host.querySelector('span[tabindex="0"]') as HTMLElement | null;
  assert(preview, "queued message exposes a keyboard-focusable preview");
  assert(preview.classList.contains("line-clamp-2"), "queue card preview is limited to two lines");
  assert(preview.style.overflow === "hidden", "queue card preview cannot take over the panel");
  assert(preview.textContent?.includes('{"ready":true}'), "preview keeps the queued code source");
  assert(!preview.textContent?.includes("Copy"), "preview omits copied code controls");
  assert(!preview.textContent?.includes("Fullscreen"), "preview omits copied fullscreen controls");

  await act(async () => {
    const event = new (globalThis as unknown as { PointerEvent: typeof MouseEvent }).PointerEvent("pointerover", {
      bubbles: true,
    }) as PointerEvent;
    Object.defineProperty(event, "pointerType", { value: "mouse" });
    preview.dispatchEvent(event);
  });
  const tooltip = document.body.querySelector('[role="tooltip"]') as HTMLElement | null;
  assert(tooltip, "hover opens a plain-text tooltip");
  assert(preview.getAttribute("aria-describedby") === tooltip.id, "preview references its accessible tooltip");
  assert(tooltip.style.maxHeight === "12rem", "tooltip height remains compact");
  assert(tooltip.style.overflow === "hidden", "tooltip never becomes an internal document scroller");
  assert(!tooltip.textContent?.includes("Copy"), "tooltip omits copied code controls");

  const send = host.querySelector('[data-action-title="Send now"]') as HTMLButtonElement | null;
  assert(send, "send-now control remains available");
  await act(async () => send.click());
  assert(sentId === message.id, "send-now behavior remains unchanged");

  const edit = host.querySelector('[data-action-title="Edit"]') as HTMLButtonElement | null;
  assert(edit, "edit control remains available");
  await act(async () => edit.click());
  const editor = host.querySelector('[data-testid="delayed-send-edit-text"]') as HTMLTextAreaElement | null;
  assert(editor?.value === originalText, "editing retains the exact queued payload rather than the preview");

  await act(async () => root.unmount());
  console.log("DelayedSendPanel tests: 1/1 passed");
}

void main();
