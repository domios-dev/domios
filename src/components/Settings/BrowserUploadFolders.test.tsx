import assert from "node:assert/strict";
import { Window } from "happy-dom";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { defaultConfig } from "../../hooks/useConfig";
import { BrowserUploadFolders } from "./BrowserUploadFolders";

const window = new Window({ url: "http://localhost/" });
for (const [name, value] of Object.entries({ window, document: window.document, navigator: window.navigator, HTMLElement: window.HTMLElement, Element: window.Element, Node: window.Node, Event: window.Event })) {
  Object.defineProperty(globalThis, name, { value, configurable: true });
}
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

async function main() {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  let config = defaultConfig();
  config.browser_upload_folders = { other: ["/other/media"] };
  let fail = false;
  const render = () => root.render(<BrowserUploadFolders config={config} onSave={async next => {
    if (fail) throw new Error("Owner authorization required");
    config = next;
    render();
  }} />);
  const button = (label: string) => {
    const found = [...host.querySelectorAll("button")].find(node => (node.getAttribute("aria-label") ?? node.textContent) === label);
    assert(found, label);
    return found;
  };
  const input = async (label: string, value: string) => act(async () => {
    const node = host.querySelector<HTMLInputElement>('input[aria-label="' + label + '"]');
    assert(node, label);
    node.value = value;
    node.dispatchEvent(new window.Event("input", { bubbles: true }) as unknown as Event);
  });
  try {
    await act(async () => render());
    assert(button("Grant folder").disabled);
    await input("Browser profile", "work");
    await input("Absolute upload folder", "/media");
    await act(async () => button("Grant folder").click());
    assert.deepEqual(config.browser_upload_folders, { other: ["/other/media"], work: ["/media"] });
    assert(host.querySelector('ul[aria-label="Granted browser upload folders"]')?.textContent?.includes("work: /media"));
    fail = true;
    await act(async () => button("Remove /media from work").click());
    assert.equal(host.querySelector('[role="alert"]')?.textContent, "Owner authorization required");
    assert.deepEqual(config.browser_upload_folders.work, ["/media"]);
    fail = false;
    await act(async () => button("Remove /media from work").click());
    assert.deepEqual(config.browser_upload_folders, { other: ["/other/media"] });
    assert(!host.querySelector('ul')?.textContent?.includes("work: /media"));
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
  console.log("browser upload Settings list: add, remove, per-profile isolation and refusal passed");
}
void main().catch(error => { console.error(error); process.exitCode = 1; });
