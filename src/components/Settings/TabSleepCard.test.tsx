import assert from "node:assert/strict";
import { Window } from "happy-dom";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { defaultConfig } from "../../hooks/useConfig";
import { TabSleepCard } from "./TabSleepCard";

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
  const updates: unknown[] = [];
  const render = () => root.render(<TabSleepCard config={config} onUpdate={(section, patch) => {
    updates.push([section, patch]);
    config = { ...config, [section]: { ...config[section], ...patch } };
    render();
  }} />);
  try {
    await act(async () => render());
    const toggle = host.querySelector<HTMLButtonElement>('[role="switch"][aria-label="Sleep idle tabs"]');
    const input = host.querySelector<HTMLInputElement>('input[type="number"]');
    assert(toggle && input);
    assert(input.closest("label")?.textContent?.includes("Idle threshold"));
    assert.equal(input.value, "60");
    await act(async () => toggle.click());
    assert.deepEqual(updates, [["tab_sleep", { enabled: false, idle_minutes: 60 }]]);
    assert.equal(input.disabled, true);
    await act(async () => toggle.click());
    assert.equal(input.disabled, false);
    assert.equal(toggle.getAttribute("aria-checked"), "true");
    await act(async () => {
      input.value = "90";
      input.dispatchEvent(new window.Event("input", { bubbles: true }) as unknown as Event);
    });
    assert.equal(updates.length, 3);
    assert.deepEqual(updates[2], ["tab_sleep", { enabled: true, idle_minutes: 90 }]);
    assert.equal(input.value, "90");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
  console.log("tab sleep settings wiring: switch updates once and projects disabled threshold passed");
}
void main().catch(error => { console.error(error); process.exitCode = 1; });
