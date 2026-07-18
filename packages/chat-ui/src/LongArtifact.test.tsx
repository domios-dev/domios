import assert from "node:assert/strict";
import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { LONG_ARTIFACT_FULL_MAX_HEIGHT, LONG_ARTIFACT_PREVIEW_MAX_HEIGHT, LongArtifact } from "./MarkdownBody";

const window = new Window({ url: "http://localhost/" });
for (const [name, value] of Object.entries({
  window,
  document: window.document,
  navigator: window.navigator,
  HTMLElement: window.HTMLElement,
  Element: window.Element,
  Node: window.Node,
})) Object.defineProperty(globalThis, name, { value, configurable: true });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const host = document.createElement("div");
document.body.appendChild(host);
const root = createRoot(host);
const text = Array.from({ length: 80 }, (_, index) => `line ${index + 1}`).join("\n");

async function run() {
  await act(async () => root.render(<LongArtifact label="System context" text={text} expanded onToggle={() => {}} />));
  const content = host.querySelector('[data-testid="long-artifact-content"]') as HTMLElement;
  assert.equal(content.style.maxHeight, `${LONG_ARTIFACT_PREVIEW_MAX_HEIGHT}px`);
  assert.equal(content.style.overflowY, "hidden");
  assert(!content.textContent?.includes("line 80"), "preview excludes the unseen tail from the DOM");

  const toggle = host.querySelector('[data-testid="long-artifact-show-all"]') as HTMLButtonElement;
  assert.equal(toggle.textContent, "Show all");
  await act(async () => toggle.click());
  assert.equal(content.style.maxHeight, `${LONG_ARTIFACT_FULL_MAX_HEIGHT}px`);
  assert.equal(content.style.overflowY, "auto");
  assert.equal(content.getAttribute("role"), "region");
  assert.equal(content.tabIndex, 0);
  assert(content.textContent?.includes("line 80"), "Show all mounts the complete artifact");

  await act(async () => toggle.click());
  assert.equal(content.style.overflowY, "hidden");
  assert(!content.textContent?.includes("line 80"), "Show preview removes the tail again");
  await act(async () => root.unmount());
  console.log("long artifact staged expansion tests: passed");
}

void run();
