import { configureDaemonFixture } from "../../test/daemonTransport.testdata";
// Run: npx tsx src/components/Sidebar/TabItem.restoreFailure.test.tsx

import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { TabItem } from "./TabItem";
import type { Tab } from "../../lib/types";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const window = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "window", { value: window, configurable: true });
Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
Object.defineProperty(globalThis, "CustomEvent", { value: window.CustomEvent, configurable: true });
configureDaemonFixture();
globalThis.fetch = async (input) => {
  const path = new URL(String(input)).pathname;
  if (path === "/api/plugins/tab-menu") return Response.json({ items: [], receiverTargets: {} });
  if (path === "/api/events/stream") return new Response(new ReadableStream({ start() {} }));
  throw new Error(`Unexpected sidebar request: ${path}`);
};
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

async function main() {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const tab = {
    id: "fixture-tab",
    pty_id: null,
    tab_type: "Terminal",
    title: "Wright",
    cwd: null,
    agent_type: null,
    role: "Manager",
    role_profile: "orchestrator",
    boot_stage: "spawn_failed",
    boot_failure_reason: "opaque-reason-32",
  } as unknown as Tab;

  await act(async () => {
    root.render(
      <TabItem
        tab={tab}
        index={0}
        isActive={false}
        onClick={() => undefined}
        onClose={() => undefined}
        canClose={false}
        workspaceId="fixture-workspace"
      />,
    );
  });

  const badge = host.querySelector('[data-testid="tab-launch-failure-badge"]');
  assert(badge?.getAttribute("role") === "img", "sidebar failure badge has an image role");
  assert(
    badge.getAttribute("aria-label") === "Wright launch failed",
    "sidebar failure badge has its own accessible name",
  );

  const renderSleeping = (frozen: boolean) => act(async () => root.render(
    <TabItem tab={{ ...tab, frozen }} index={0} isActive={false} onClick={() => undefined}
      onClose={() => undefined} canClose={false} workspaceId="fixture-workspace" />,
  ));
  await renderSleeping(true);
  assert(host.querySelector('[role="img"][aria-label="Sleeping"]'), "sleeping snapshot exposes the accessible indicator");
  await renderSleeping(false);
  assert(!host.querySelector('[aria-label="Sleeping"]'), "waking clears the sleeping indicator");

  await act(async () => root.unmount());
  host.remove();
  console.log("✓ TabItem restore failure wiring: sidebar badge role and accessible name");
}

void main();
