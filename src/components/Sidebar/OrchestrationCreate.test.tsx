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

async function selectValue(el: Element | null | undefined, value: string) {
  assert(el, "select target must exist");
  const select = el as HTMLSelectElement;
  await act(async () => {
    select.value = value;
    select.dispatchEvent(new Event("change", { bubbles: true }));
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
    preserves_context_on_resume: false,
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
  const selects = Array.from(host.querySelectorAll("select"));
  // 2 rows x (provider select + model select) = 4 selects, plus the title text input separately.
  assert(selects.length === 4, `expected 4 selects (2 provider + 2 model), got ${selects.length}`);
  const providerOptionLabels = Array.from(selects[0].querySelectorAll("option")).map((o) => o.textContent);
  assert(providerOptionLabels.includes("claude"), "orchestrator provider select lists claude");
  assert(providerOptionLabels.includes("codex"), "orchestrator provider select lists codex");
  const titleInput = host.querySelector('input[type="text"]') as HTMLInputElement | null;
  assert(titleInput, "renders an optional plan-title text input");
  assert(titleInput.placeholder === "Orchestration", "title input has a sensible placeholder");
  await act(async () => root.unmount());
});

test("defaults each role's model to the provider's default_model", async () => {
  const { host, root } = await render(
    <OrchestrationCreate providers={PROVIDERS} onCancel={() => {}} onConfirm={() => {}} />,
  );
  const selects = Array.from(host.querySelectorAll("select")) as HTMLSelectElement[];
  const [, orchestratorModelSelect] = selects;
  assert(orchestratorModelSelect.value === "sonnet", `expected default model "sonnet", got ${orchestratorModelSelect.value}`);
  await act(async () => root.unmount());
});

test("switching a role's provider resets that role's model to the new default", async () => {
  const { host, root } = await render(
    <OrchestrationCreate providers={PROVIDERS} onCancel={() => {}} onConfirm={() => {}} />,
  );
  const selects = Array.from(host.querySelectorAll("select")) as HTMLSelectElement[];
  const [orchestratorProviderSelect, orchestratorModelSelect] = selects;
  await selectValue(orchestratorProviderSelect, "codex");
  assert(orchestratorModelSelect.value === "gpt-5", `expected model reset to codex default, got ${orchestratorModelSelect.value}`);
  await act(async () => root.unmount());
});

// Title is plain useState wired via a controlled <input>; simulating a typed
// keystroke through happy-dom's dispatchEvent does not reach React's onChange
// in this harness (a known rough edge — the same native-setter workaround is
// defined-but-unused elsewhere in this codebase for the same reason). Provider
// and model selections use <select>, which React's harness handles reliably,
// so this test covers those plus the title's untouched (undefined) default.
test("confirm calls onConfirm with both roles' selections and an unset title by default", async () => {
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
  const selects = Array.from(host.querySelectorAll("select")) as HTMLSelectElement[];
  const [, , plannerProviderSelect] = selects;
  await selectValue(plannerProviderSelect, "codex");

  const buttons = Array.from(host.querySelectorAll("button"));
  const createButton = buttons.find((b) => b.textContent === "Create");
  await click(createButton);

  assert(result !== null, "onConfirm should have been called");
  const r = result as unknown as OrchestrationCreateResult;
  assert(r.orchestratorAgent === "claude", `expected orchestrator claude, got ${r.orchestratorAgent}`);
  assert(r.orchestratorModel === "sonnet", `expected orchestrator model sonnet, got ${r.orchestratorModel}`);
  assert(r.plannerAgent === "codex", `expected planner codex, got ${r.plannerAgent}`);
  assert(r.plannerModel === "gpt-5", `expected planner model gpt-5, got ${r.plannerModel}`);
  assert(r.title === undefined, `expected unset title by default, got ${r.title}`);
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
