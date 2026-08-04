import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { FrontendCrashBoundary } from "./FrontendCrashBoundary";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const testWindow = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "window", { value: testWindow, configurable: true });
Object.defineProperty(globalThis, "document", { value: testWindow.document, configurable: true });
Object.defineProperty(globalThis, "HTMLElement", { value: testWindow.HTMLElement, configurable: true });
Object.defineProperty(globalThis, "Element", { value: testWindow.Element, configurable: true });
Object.defineProperty(globalThis, "Node", { value: testWindow.Node, configurable: true });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function BrokenView(): React.ReactNode {
  throw new Error("render exploded");
}

async function main() {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const originalError = console.error;
  console.error = () => {};
  await act(async () => {
    root.render(
      <FrontendCrashBoundary>
        <BrokenView />
      </FrontendCrashBoundary>,
    );
  });
  console.error = originalError;

  assert(host.querySelector('[role="alert"]'), "render errors show an in-app crash surface");
  assert(host.textContent?.includes("render exploded"), "crash surface includes the actionable error");
  assert(host.querySelector('[data-tauri-drag-region]'), "crash surface keeps a draggable titlebar");
  assert(host.textContent?.includes("Reload interface"), "crash surface offers reload");
  console.log("✓ frontend render errors keep usable CodeTerm chrome");
}

void main();
