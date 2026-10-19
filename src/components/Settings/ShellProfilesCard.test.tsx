import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { __setInvokeForTest } from "../../lib/bridge";
import { defaultConfig } from "../../hooks/useConfig";
import { ShellProfilesCard } from "./ShellProfilesCard";

const window = new Window({ url: "http://localhost/" });
for (const [name, value] of Object.entries({ window, document: window.document, navigator: window.navigator, HTMLElement: window.HTMLElement, Element: window.Element, Node: window.Node, Event: window.Event })) {
  Object.defineProperty(globalThis, name, { value, configurable: true });
}
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

async function main() {
  const requests: unknown[] = [];
  let failInitial = true;
  let failRefresh = false;
  __setInvokeForTest(async <T,>(command: string, args?: unknown): Promise<T> => {
    assert.equal(command, "shell_profiles");
    requests.push(args);
    if ((args as { refresh?: boolean } | undefined)?.refresh === false && failInitial) throw new Error("opaque background shell scan failure");
    if ((args as { refresh?: boolean } | undefined)?.refresh === true && failRefresh) throw new Error("opaque explicit shell scan failure");
    return [{ id: "wsl:Ubuntu", name: "WSL: Ubuntu", executable: "wsl.exe", args: [], wsl_distro: "Ubuntu" }] as T;
  });
  const quietHost = document.createElement("div");
  document.body.appendChild(quietHost);
  const quietRoot = createRoot(quietHost);
  await act(async () => quietRoot.render(<ShellProfilesCard config={defaultConfig()} onUpdate={() => {}} />));
  assert.equal(quietHost.querySelector('[role="alert"]'), null, "automatic shell detection failure does not create an alert");
  await act(async () => quietRoot.unmount());
  quietHost.remove();
  failInitial = false;

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const updates: unknown[] = [];
  try {
    await act(async () => root.render(<ShellProfilesCard config={defaultConfig()} onUpdate={(section, patch) => updates.push([section, patch])} />));
    const select = host.querySelector("select");
    assert(select);
    assert(host.querySelector("label")?.textContent?.includes("Default shell"));
    assert(select.querySelector('option[value="wsl:Ubuntu"]'));
    assert.equal(select.value, "");
    await act(async () => {
      select.value = "wsl:Ubuntu";
      select.dispatchEvent(new window.Event("change", { bubbles: true }) as unknown as Event);
    });
    assert.deepEqual(updates, [["terminal", { default_shell: "wsl:Ubuntu" }]]);
    const refresh = host.querySelector<HTMLButtonElement>("button");
    assert(refresh);
    await act(async () => refresh.click());
    failRefresh = true;
    await act(async () => refresh.click());
    assert.equal(host.querySelectorAll('[role="alert"]').length, 1, "failed explicit shell refresh creates one alert");
    assert.deepEqual(requests, [{ refresh: false }, { refresh: false }, { refresh: true }, { refresh: true }]);
  } finally {
    await act(async () => root.unmount());
    host.remove();
    __setInvokeForTest(null);
  }
  console.log("shell profile settings wiring: passed");
}

void main().catch((error) => { console.error(error); process.exit(1); });
