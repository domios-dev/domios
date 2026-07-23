// Run: npx tsx src/components/Sidebar/OrchestrationCreate.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { OrchestrationCreate, type OrchestrationCreateResult } from "./OrchestrationCreate";
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
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLButtonElement", { value: window.HTMLButtonElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLSelectElement", { value: window.HTMLSelectElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLInputElement", { value: window.HTMLInputElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
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

async function click(el: Element | null | undefined) {
  assert(el, "click target must exist");
  await act(async () => {
    (el as HTMLElement).click();
  });
}

async function openDropdown(trigger: Element | null | undefined): Promise<void> {
  assert(trigger, "dropdown trigger must exist");
  await act(async () => {
    trigger.dispatchEvent(new window.Event("pointerdown", { bubbles: true, cancelable: true }));
  });
}

async function selectDropdownValue(testId: string, value: string) {
  const trigger = document.querySelector(`[data-testid="${testId}"]`);
  await openDropdown(trigger);
  const option = document.querySelector(`[data-testid="${testId}-option-${value}"]`);
  assert(option, `dropdown option ${testId}/${value} must exist`);
  await act(async () => {
    (option as HTMLButtonElement).click();
  });
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
  } as ProviderDescriptor;
}

const PROVIDERS = [
  provider("claude", [
    { id: "sonnet", display_name: "Sonnet" },
    { id: "fable", display_name: "Fable" },
  ]),
  provider("codex", [{ id: "gpt-5", display_name: "GPT-5" }]),
];

setupDom();

test("renders an Orchestrator row and a Planner row with provider options", async () => {
  const { host, root } = await render(
    <OrchestrationCreate providers={PROVIDERS} onCancel={() => {}} onConfirm={() => {}} />,
  );
  assert(host.textContent?.includes("Orchestrator"), "shows an Orchestrator row label");
  assert(host.textContent?.includes("Planner"), "shows a Planner row label");
  assert(host.querySelector("select") === null, "uses DropdownSelect, not native selects");
  const providerTrigger = host.querySelector('[data-testid="orchestration-create-orchestrator-provider"]');
  assert(providerTrigger, "orchestrator provider dropdown exists");
  await openDropdown(providerTrigger);
  assert(document.querySelector('[data-testid="orchestration-create-orchestrator-provider-option-claude"]'), "provider dropdown lists claude");
  assert(document.querySelector('[data-testid="orchestration-create-orchestrator-provider-option-codex"]'), "provider dropdown lists codex");
  assert(host.textContent?.includes("wait for your first goal"), "explains that no plan is created at spawn");
  await act(async () => root.unmount());
});

test("defaults each role's model to the provider's default_model", async () => {
  const { host, root } = await render(
    <OrchestrationCreate providers={PROVIDERS} onCancel={() => {}} onConfirm={() => {}} />,
  );
  const orchestratorModel = host.querySelector('[data-testid="orchestration-create-orchestrator-model"]') as HTMLButtonElement | null;
  assert(orchestratorModel?.dataset.value === "sonnet", `expected default model "sonnet", got ${orchestratorModel?.dataset.value}`);
  await act(async () => root.unmount());
});

test("switching a role's provider resets that role's model to the new default", async () => {
  const { host, root } = await render(
    <OrchestrationCreate providers={PROVIDERS} onCancel={() => {}} onConfirm={() => {}} />,
  );
  await selectDropdownValue("orchestration-create-orchestrator-provider", "codex");
  const orchestratorModel = host.querySelector('[data-testid="orchestration-create-orchestrator-model"]') as HTMLButtonElement | null;
  assert(orchestratorModel?.dataset.value === "gpt-5", `expected model reset to codex default, got ${orchestratorModel?.dataset.value}`);
  await act(async () => root.unmount());
});

test("confirm calls onConfirm with both role selections", async () => {
  let result: OrchestrationCreateResult | null = null;
  const { host, root } = await render(
    <OrchestrationCreate
      providers={PROVIDERS}
      onCancel={() => {}}
      onConfirm={(r) => {
        result = r;
      }}
    />,
  );
  await selectDropdownValue("orchestration-create-planner-provider", "codex");

  const buttons = Array.from(host.querySelectorAll("button"));
  const createButton = buttons.find((b) => b.textContent === "Create");
  await click(createButton);

  assert(result !== null, "onConfirm should have been called");
  const r = result as unknown as OrchestrationCreateResult;
  assert(r.orchestratorAgent === "claude", `expected orchestrator claude, got ${r.orchestratorAgent}`);
  assert(r.orchestratorModel === "sonnet", `expected orchestrator model sonnet, got ${r.orchestratorModel}`);
  assert(r.plannerAgent === "codex", `expected planner codex, got ${r.plannerAgent}`);
  assert(r.plannerModel === "gpt-5", `expected planner model gpt-5, got ${r.plannerModel}`);
  await act(async () => root.unmount());
});

test("cancel invokes onCancel without calling onConfirm", async () => {
  let confirmed = false;
  let cancelled = false;
  const { host, root } = await render(
    <OrchestrationCreate
      providers={PROVIDERS}
      onCancel={() => {
        cancelled = true;
      }}
      onConfirm={() => {
        confirmed = true;
      }}
    />,
  );
  const buttons = Array.from(host.querySelectorAll("button"));
  const cancelButton = buttons.find((b) => b.textContent === "Cancel");
  await click(cancelButton);
  assert(cancelled, "onCancel should fire");
  assert(!confirmed, "onConfirm should not fire on cancel");
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
  console.log(`OrchestrationCreate tests: ${tests.length - failed}/${tests.length} passed`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
