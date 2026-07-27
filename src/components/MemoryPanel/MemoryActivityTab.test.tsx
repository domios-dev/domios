import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { setAgentDescriptors } from "../../lib/agentDescriptorCache";
import type { GraphWorkerInfo, ProviderDescriptor } from "../../lib/types";
import { WorkerCard } from "./MemoryActivityTab";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
}

async function render(ui: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(ui));
  return { host, root };
}

function provider(id: string, displayName: string): ProviderDescriptor {
  return {
    schema_version: 1,
    id,
    display_name: displayName,
    transport: "pty",
    system_prompt_delivery: "external",
    available_models: [],
    default_model: undefined,
    settings_schema: [],
    notifications_schema: [],
    supports_lazy_skills: false,
    has_hooks: false,
    icon_hint: "◆",
  };
}

const worker: GraphWorkerInfo = {
  worker_id: "worker-1",
  tab_id: "tab-1",
  intent: "extract: provider metadata",
  spawned_by: "tab-parent",
  spawned_at: Date.now(),
  status: "done",
  progress_text: null,
  touched_nodes: [],
  color: "#fff",
  report: null,
  done_at: Date.now(),
  agent_provider: "cursor",
  model: "not-a-provider",
};

async function main() {
  setupDom();
  setAgentDescriptors([provider("cursor", "Cursor")]);
  const { host, root } = await render(<WorkerCard worker={worker} onOpen={() => {}} onKill={null} />);
  const icon = host.querySelector('[role="img"]');
  assert(icon?.getAttribute("title") === "Executed by cursor", "icon tooltip must name the durable worker provider");
  assert(icon?.getAttribute("aria-label") === "Executed by cursor", "icon must expose its executor provider to assistive technology");
  assert(icon?.textContent === "◆", "provider glyph must come from the provider descriptor, not the model text");
  await act(async () => root.unmount());
  console.log("✓ Memory worker card renders its durable executor provider icon");
}

void main();
