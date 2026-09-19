// Run: npx tsx src/components/Sidebar/OrchestrationPairForm.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { OrchestrationPairForm, type OrchestrationPairValues } from "./OrchestrationPairForm";
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
  // happy-dom lacks rAF; DropdownMenu schedules a reposition through it on open.
  if (typeof (globalThis as { requestAnimationFrame?: unknown }).requestAnimationFrame !== "function") {
    (globalThis as { requestAnimationFrame: (cb: FrameRequestCallback) => number }).requestAnimationFrame = (
      cb: FrameRequestCallback,
    ) => { cb(0); return 0; };
    (globalThis as { cancelAnimationFrame: (id: number) => void }).cancelAnimationFrame = () => {};
  }
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
    cli_binary: id,
  } as ProviderDescriptor;
}

const PROVIDERS = [
  provider("provider-a", [
    { id: "model-a", display_name: "Model A" },
    { id: "model-b", display_name: "Model B" },
  ]),
  provider("provider-b", [{ id: "model-c", display_name: "Model C" }]),
];

setupDom();

/** Opens a DropdownSelect by dispatching the pointerdown its trigger listens
 *  for (happy-dom has no PointerEvent; a bubbling Event of that type still
 *  fires React's native pointerdown listener). Options then render in a portal
 *  under document.body. */
async function openDropdown(trigger: Element): Promise<void> {
  await act(async () => {
    trigger.dispatchEvent(new window.Event("pointerdown", { bubbles: true, cancelable: true }));
  });
}

async function click(element: Element | null | undefined): Promise<void> {
  assert(element, "expected element to click");
  await act(async () => (element as HTMLButtonElement).click());
}

test("card layout renders PairAffordance with orchestrator and planner rows", async () => {
  const { host, root } = await render(
    <OrchestrationPairForm
      providers={PROVIDERS}
      layout="card"
      values={{
        orchestratorProviderId: "provider-a",
        orchestratorModelId: "model-a",
        plannerProviderId: "provider-a",
        plannerModelId: "model-b",
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

test("disabled providers render as disabled options in the dropdown", async () => {
  const { host, root } = await render(
    <OrchestrationPairForm
      providers={PROVIDERS}
      layout="card"
      values={{
        orchestratorProviderId: "provider-a",
        orchestratorModelId: "model-a",
        plannerProviderId: "provider-a",
        plannerModelId: "model-b",
      }}
      disabledProviderIds={new Set(["provider-b"])}
      onChange={() => {}}
      roleTestIds={{ orchestratorProvider: "orch-provider" }}
    />,
  );
  // No native <select> anywhere — the flicker-prone native popup is gone.
  assert(host.querySelector("select") === null, "no native select elements");
  const providerTrigger = host.querySelector('[data-testid="orch-provider"]');
  assert(providerTrigger, "orchestrator provider trigger exists");
  await openDropdown(providerTrigger!);
  const disabledOption = document.querySelector('[data-testid="orch-provider-option-provider-b"]') as HTMLButtonElement | null;
  assert(disabledOption?.disabled === true, "unavailable provider option is disabled");
  assert(disabledOption?.textContent?.includes("not installed"), "unavailable provider is marked not installed");
  await act(async () => root.unmount());
});

test("model selector entries match the provider catalog exactly", async () => {
  const { host, root } = await render(
    <OrchestrationPairForm
      providers={PROVIDERS}
      values={{
        orchestratorProviderId: "provider-a",
        orchestratorModelId: "model-a",
        plannerProviderId: "provider-a",
        plannerModelId: "model-b",
      }}
      onChange={() => {}}
      roleTestIds={{ orchestratorModel: "orch-model" }}
    />,
  );
  const trigger = host.querySelector('[data-testid="orch-model"]');
  assert(trigger, "orchestrator model trigger exists");
  await openDropdown(trigger);
  const optionValues = Array.from(document.querySelectorAll('[data-testid^="orch-model-option-"]'))
    .map((option) => option.getAttribute("data-testid")?.replace("orch-model-option-", ""));
  assert(optionValues.join(",") === "model-a,model-b", `model entries must equal catalog, got ${optionValues}`);
  await act(async () => root.unmount());
});

test("choosing a reasoning effort reports it through onChange", async () => {
  const providerWithEfforts = {
    ...PROVIDERS[0],
    reasoning_efforts: [
      { id: "low", display_name: "Low" },
      { id: "high", display_name: "High" },
    ],
    default_reasoning_effort: "low",
  } as ProviderDescriptor;
  const changes: OrchestrationPairValues[] = [];
  const { host, root } = await render(
    <OrchestrationPairForm
      providers={[providerWithEfforts]}
      layout="stack"
      values={{
        orchestratorProviderId: "provider-a",
        orchestratorModelId: "model-a",
        orchestratorEffortId: "low",
        plannerProviderId: "provider-a",
        plannerModelId: "model-b",
        plannerEffortId: "low",
      }}
      onChange={(next) => { changes.push(next); }}
      roleTestIds={{ orchestratorEffort: "orch-effort" }}
    />,
  );

  const trigger = host.querySelector('[data-testid="orch-effort"]');
  assert(trigger, "effort selector renders for a provider that declares efforts");
  await openDropdown(trigger!);
  const high = document.querySelector('[data-testid="orch-effort-option-high"]');
  assert(high, "declared effort renders as an option");
  await click(high);
  const latest = changes[changes.length - 1];
  assert(
    latest?.orchestratorEffortId === "high",
    `selected effort must reach onChange, got ${latest?.orchestratorEffortId}`,
  );
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
