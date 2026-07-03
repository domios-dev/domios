// Run: npx tsx src/components/Sidebar/OrchestrationPairForm.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { OrchestrationPairForm } from "./OrchestrationPairForm";
import type { ProviderDescriptor } from "../../lib/types";

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
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLSelectElement", { value: window.HTMLSelectElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
}

async function render(ui: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  return { host, root };
}

function provider(id: string, models: Array<{ id: string; display_name: string }>): ProviderDescriptor {
  return {
    schema_version: 1,
    id,
    display_name: id,
    transport: "pty",
    system_prompt_delivery: "external",
    available_models: models.map((m) => ({ id: m.id, display_name: m.display_name })),
    default_model: models[0]?.id,
    settings_schema: [],
    notifications_schema: [],
    supports_lazy_skills: false,
    has_hooks: false,
    preserves_context_on_resume: false,
    cli_binary: id,
  } as ProviderDescriptor;
}

const PROVIDERS = [
  provider("claude", [
    { id: "opus", display_name: "Opus" },
    { id: "fable", display_name: "Fable" },
  ]),
  provider("codex", [{ id: "gpt-5", display_name: "GPT-5" }]),
];

setupDom();

test("card layout renders PairAffordance with equal orchestrator and planner rows", async () => {
  const { host, root } = await render(
    <OrchestrationPairForm
      providers={PROVIDERS}
      layout="card"
      values={{
        orchestratorProviderId: "claude",
        orchestratorModelId: "opus",
        plannerProviderId: "claude",
        plannerModelId: "fable",
      }}
      onChange={() => {}}
    />,
  );
  assert(host.querySelector('[data-pair-affordance-layout="card"]'), "uses card layout");
  assert(host.querySelector('[data-pair-role="orchestrator"]'), "orchestrator slot exists");
  assert(host.querySelector('[data-pair-role="planner"]'), "planner slot exists");
  assert(host.textContent?.includes("Orchestrator"), "orchestrator label visible");
  assert(host.textContent?.includes("Planner"), "planner label visible");
  await act(async () => root.unmount());
});

test("disabled providers render as disabled options", async () => {
  const { host, root } = await render(
    <OrchestrationPairForm
      providers={PROVIDERS}
      layout="card"
      values={{
        orchestratorProviderId: "claude",
        orchestratorModelId: "opus",
        plannerProviderId: "claude",
        plannerModelId: "fable",
      }}
      disabledProviderIds={new Set(["codex"])}
      onChange={() => {}}
    />,
  );
  const codexOption = Array.from(host.querySelectorAll("option")).find((o) => o.textContent?.includes("codex"));
  assert(codexOption?.disabled === true, "codex option disabled");
  await act(async () => root.unmount());
});

async function main() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`✓ ${name}`);
    } catch (error) {
      failed += 1;
      console.error(`✗ ${name}`);
      console.error(error);
    }
  }
  console.log(`OrchestrationPairForm tests: ${tests.length - failed}/${tests.length} passed`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
