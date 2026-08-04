// Run: npx tsx src/components/MeshPanel/AddPeerPanel.test.tsx

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { __setInvokeForTest } from "../../lib/bridge";
import type { MeshPeersResponse } from "../../lib/types";
import AddPeerPanel from "./AddPeerPanel";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const peerId = "peer-1";
const peers: MeshPeersResponse = {
  paired: [],
  incoming_requests: [],
  local_instance_name: "Local PC",
  discovered: [{
    instance_id: peerId,
    instance_name: "Remote Mac",
    addresses: [{ ip: "100.64.0.2", is_tailscale: true }],
    api_port: 7685,
    discovery_method: "tailscale",
    last_seen: 0,
    dev: false,
    mesh_id: null,
    vouched_by: [],
  }],
};

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://127.0.0.1/mesh" });
  for (const key of ["window", "document", "HTMLElement", "HTMLButtonElement", "navigator"]) {
    Object.defineProperty(globalThis, key, {
      value: (window as unknown as Record<string, unknown>)[key],
      configurable: true,
    });
  }
}

async function render(connect: () => Promise<unknown>): Promise<{ host: HTMLDivElement; root: Root }> {
  __setInvokeForTest(async <T,>(command: string): Promise<T> => {
    if (command === "mesh_list_peers") return peers as T;
    if (command === "get_config") return { mesh: { enabled: true } } as T;
    if (command === "mesh_connect") return (await connect()) as T;
    throw new Error(`unexpected invoke: ${command}`);
  });
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(<AddPeerPanel />);
  });
  return { host, root };
}

async function connect(host: HTMLDivElement) {
  const button = host.querySelector(`[data-testid="mesh-connect-${peerId}"]`) as HTMLButtonElement;
  assert(button, "connect button present");
  await act(async () => button.click());
}

test("surfaces an existing pending pairing request", async () => {
  const { host, root } = await render(async () => ({ status: "pending", existing: true }));
  await connect(host);
  const status = host.querySelector(`[data-testid="mesh-connect-status-${peerId}"]`);
  assert(
    status?.textContent === "Pairing request is already pending.",
    "pending status is visible",
  );
  assert(status?.getAttribute("role") === "status" && status.getAttribute("aria-live") === "polite", "pending announcement is polite");
  await act(async () => root.unmount());
});

test("surfaces an already-connected result", async () => {
  const { host, root } = await render(async () => ({ status: "connected" }));
  await connect(host);
  const status = host.querySelector(`[data-testid="mesh-connect-status-${peerId}"]`);
  assert(
    status?.textContent === "Already connected.",
    "connected status is visible",
  );
  assert(status?.getAttribute("role") === "status" && status.getAttribute("aria-live") === "polite", "connected announcement is polite");
  await act(async () => root.unmount());
});

test("surfaces a connect error", async () => {
  const { host, root } = await render(async () => { throw new Error("network unreachable"); });
  await connect(host);
  const error = host.querySelector('[data-testid="mesh-connect-error"]');
  assert(
    error?.textContent?.includes("network unreachable"),
    "connect error is visible",
  );
  assert(error?.getAttribute("role") === "alert", "connect error announces urgently");
  await act(async () => root.unmount());
});

async function main() {
  setupDom();
  let passed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      passed++;
      console.log(`  ✓ ${name}`);
    } catch (error) {
      console.error(`  ✗ ${name}`);
      console.error(error instanceof Error ? error.stack : error);
      process.exit(1);
    }
  }
  __setInvokeForTest(null);
  console.log(`\n✓ ${passed}/${tests.length} AddPeerPanel tests passed`);
}

main();
