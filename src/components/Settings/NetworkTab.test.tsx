// Run: npx tsx src/components/Settings/NetworkTab.test.tsx

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { __setInvokeForTest } from "../../lib/bridge";
import { NetworkTab } from "./NetworkTab";
import { configureDaemonTransport } from "../../lib/daemonBootstrap";
import { clearRelayTarget, setRelayTarget } from "../../lib/webApi";

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  for (const key of ["window", "document", "navigator", "HTMLElement", "HTMLButtonElement", "Element", "Node", "Event", "localStorage"]) {
    Object.defineProperty(globalThis, key, { value: window[key as keyof Window], configurable: true });
  }
}

async function render(): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(<NetworkTab />));
  return { host, root };
}

async function flush() {
  await act(async () => {});
}

async function withInvoke(
  invoke: (command: string) => Promise<unknown>,
  body: () => Promise<void>,
) {
  __setInvokeForTest(async <T,>(command: string) => invoke(command) as Promise<T>);
  try { await body(); } finally { __setInvokeForTest(null); }
}

setupDom();

async function statusMocks(command: string): Promise<unknown> {
  if (command === "get_config") return { api: { port: 7685 } };
  if (command === "get_network_info") return { api_port: 7685 };
  if (command === "get_serve_status") return null;
  throw new Error(`unexpected command: ${command}`);
}

async function testSuccessfulRestart() {
  let statusReads = 0;
  await withInvoke(async (command) => {
    if (command === "get_rest_api_status") {
      statusReads++;
      return { generation: 4, phase: "failed", message: "port occupied" };
    }
    if (command === "restart_rest_api") return { generation: 6, phase: "listening", message: null };
    return statusMocks(command);
  }, async () => {
    const { host, root } = await render();
    assert(host.textContent?.includes("port occupied"), "failure message must be visible");
    assert(host.querySelector('[role="alert"]'), "failure must be announced as an alert");
    const button = host.querySelector("button") as HTMLButtonElement | null;
    assert(button?.textContent === "Restart REST API", "restart action must have an accessible name");
    await act(async () => button.click());
    await flush();
    assert(!host.textContent?.includes("port occupied"), "newer listening generation clears degraded status");
    await act(async () => root.unmount());
  });
}

async function testRestartFailureRemainsActionable() {
  await withInvoke(async (command) => {
    if (command === "get_rest_api_status") return { generation: 4, phase: "failed", message: "port occupied" };
    if (command === "restart_rest_api") throw new Error("still occupied");
    return statusMocks(command);
  }, async () => {
    const { host, root } = await render();
    const button = host.querySelector("button") as HTMLButtonElement | null;
    assert(button, "restart action must be rendered for a failed listener");
    await act(async () => button.click());
    await flush();
    assert(host.querySelector("button")?.textContent === "Restart REST API", "failed restart remains actionable");
    await act(async () => root.unmount());
  });
}

async function testWebAccessUsesAuthenticatedCredential() {
  configureDaemonTransport({ origin: window.location.origin, token: "transport-credential" });
  await withInvoke(async (command) => {
    if (command === "get_rest_api_status") return { generation: 1, phase: "listening" };
    return statusMocks(command);
  }, async () => {
    const { host, root } = await render();
    const show = Array.from(host.querySelectorAll("button")).find((button) => button.textContent === "Show");
    assert(show, "credential reveal must be available");
    await act(async () => show.click());
    assert(host.textContent?.includes("transport-credential"), "web access uses authenticated transport when config contains no credential");
    await act(async () => root.unmount());
  });
  setRelayTarget("other-machine", "Other machine");
  try {
    await withInvoke(async (command) => {
      if (command === "get_rest_api_status") return { generation: 1, phase: "listening" };
      return statusMocks(command);
    }, async () => {
      const { host, root } = await render();
      const show = Array.from(host.querySelectorAll("button")).find((button) => button.textContent === "Show");
      assert(show, "reveal action stays available");
      await act(async () => show.click());
      assert(!host.textContent?.includes("transport-credential"), "a peer panel cannot present the gateway credential as its own");
      await act(async () => root.unmount());
    });
  } finally { clearRelayTarget(); }
}

async function main() {
  await testWebAccessUsesAuthenticatedCredential();
  await testSuccessfulRestart();
  await testRestartFailureRemainsActionable();
  console.log("PASS NetworkTab REST listener recovery");
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
