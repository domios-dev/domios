// Run: npx tsx src/components/Sidebar/TabItem.general-agent.test.tsx

import React, { act } from "react";
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
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function tab(overrides: Partial<Tab>): Tab {
  return {
    id: "tab",
    pty_id: null,
    tab_type: "Terminal",
    title: "Main",
    cwd: null,
    ...overrides,
  };
}

async function renderTab(value: Tab) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <TabItem
        tab={value}
        index={0}
        isActive={false}
        onClick={() => {}}
        onClose={() => {}}
        canClose={false}
        workspaceId="general"
      />,
    );
  });
  return { host, root };
}

const general = await renderTab(tab({ machine_agent_identity: "general_agent", protected: true }));
assert(
  general.host.querySelector('[data-testid="tab-subline"]')?.textContent === "General Agent",
  "General subtitle is exact",
);
assert(
  general.host.querySelector('button[aria-label="General Agent"]'),
  "General activation has an exact accessible name",
);
assert(!general.host.textContent?.includes("Orchestrator"), "General row never says Orchestrator");
await act(async () => general.root.unmount());

const orchestrator = await renderTab(tab({ title: "Lead", role: "Orchestrator" }));
assert(
  orchestrator.host.querySelector('[data-testid="tab-subline"]')?.textContent === "ORCHESTRATOR",
  "Team Orchestrator label is unchanged",
);
await act(async () => orchestrator.root.unmount());
