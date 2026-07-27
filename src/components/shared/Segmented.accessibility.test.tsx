// Run: npx tsx src/components/shared/Segmented.accessibility.test.tsx

import { act } from "react";
import { Window } from "happy-dom";
import { createRoot } from "react-dom/client";
import { Segmented } from "./Segmented";

const window = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "window", { value: window, configurable: true });
Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });

async function main() {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);

  await act(async () => {
    root.render(
      <Segmented
        value="chat"
        options={[
          { value: "chat", label: "Chat" },
          { value: "terminal", label: "Terminal" },
        ]}
        onChange={() => {}}
        data-testid="tab-mode-switch"
        data-tab-id="tab-a"
        aria-label="Tab view"
      />,
    );
  });

  const switcher = host.querySelector('[data-testid="tab-mode-switch"]');
  if (switcher?.getAttribute("role") !== "tablist") throw new Error("switch is a tablist");
  if (switcher?.getAttribute("aria-label") !== "Tab view") throw new Error("switch is named");
  if (switcher?.getAttribute("data-tab-id") !== "tab-a") throw new Error("switch exposes Tab scope");
  const chat = host.querySelector('[role="tab"][data-value="chat"]');
  const terminal = host.querySelector('[role="tab"][data-value="terminal"]');
  if (chat?.getAttribute("aria-selected") !== "true") throw new Error("active view is exposed");
  if (terminal?.getAttribute("aria-selected") !== "false") throw new Error("inactive view is exposed");

  await act(async () => root.unmount());
  console.log("ok - segmented accessibility contract");
}

void main();
