// Run: npx tsx src/components/MeshPanel/PeerTerminal.test.tsx

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { peerInventoryModel } from "./peerInventoryModel";
import { daemon, schedule } from "../General/generalOverview.testdata";
import { PeerTerminal } from "./PeerTerminal";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://127.0.0.1/peer-terminal" });
  for (const key of ["window", "document", "HTMLElement", "HTMLButtonElement"]) {
    Object.defineProperty(globalThis, key, {
      value: (window as unknown as Record<string, unknown>)[key],
      configurable: true,
    });
  }
}

async function render(props: Parameters<typeof PeerTerminal>[0]): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(<PeerTerminal {...props} />);
  });
  return { host, root };
}

const base = {
  name: "rollacode-bot",
  ip: "100.64.0.2",
  isPaired: true,
  accessLevel: "full" as const,
};

test("machine cards select the peer without a duplicate web action", async () => {
  let selected = 0;
  const { host, root } = await render({ ...base, onClick: () => { selected++; } });
  assert(host.querySelectorAll("button").length === 0, "no duplicate web action");
  await act(async () => (host.firstElementChild as HTMLElement).click());
  assert(selected === 1, "peer selected once");
  await act(async () => root.unmount());
});

test("machine cards expose real service and cron summaries", async () => {
  const inventory = peerInventoryModel([daemon, schedule], { instance_id: daemon.source_peer_id!, name: daemon.machine_name!, online: true, services_loaded: true });
  const { host, root } = await render({ ...base, inventory });
  assert(host.querySelector('section[aria-label="Services"] ul')?.textContent?.includes(daemon.name), "map Services wires the daemon name");
  assert(host.querySelector('section[aria-label="Crons"] ul')?.textContent?.includes(schedule.name), "map Crons wires the schedule name");
  assert(host.querySelector('[role="button"]')?.getAttribute("aria-label") === `Open ${base.name}`, "peer is keyboard accessible");
  await act(async () => root.unmount());
});

test("stale map capsules expose names and state without error announcements", async () => {
  const inventory = peerInventoryModel([daemon, schedule], { instance_id: daemon.source_peer_id!, name: daemon.machine_name!, online: false, services_loaded: true }, false, false, "failed request");
  let selected = 0;
  const { host, root } = await render({ ...base, inventory, offline: true, onClick: () => { selected++; } });
  assert(host.querySelector(`[role="img"][aria-label="${daemon.name}: ${inventory.services[0].stateLabel}"]`), "daemon capsule exposes name and state");
  assert(host.querySelector(`[role="img"][aria-label="${schedule.name}: ${inventory.crons[0].stateLabel}"]`), "cron capsule exposes name and state");
  assert(!host.querySelector('[role="status"], [role="alert"]'), "map inventory has no error announcement");
  await act(async () => (host.querySelector('[role="button"][aria-label="Open rollacode-bot"]') as HTMLElement).click());
  assert(selected === 1, "open action fires once");
  await act(async () => root.unmount());
});

test("a discovered-not-paired peer never renders as paired (codeterm#440)", async () => {
  const { host, root } = await render({
    ...base,
    isPaired: false,
    pending: true,
    accessLevel: "full",
  });
  assert(!host.textContent?.includes("paired ·"), "must not render the paired status line");
  assert(host.textContent?.includes("pending · auto-pairs when online") === true, "shows the pending affordance instead");
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
    } catch (e) {
      console.error(`  ✗ ${name}`);
      console.error(`    ${e instanceof Error ? e.stack : e}`);
      process.exit(1);
    }
  }
  console.log(`\n✓ ${passed}/${tests.length} PeerTerminal tests passed`);
}

main();
