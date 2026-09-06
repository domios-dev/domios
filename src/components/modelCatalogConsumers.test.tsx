// Same-descriptor oracle: New Session single/pair roles and Team add-member
// must render exactly one injected ProviderDescriptor — ids, labels, order,

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { OrchestrationPairForm, OrchestrationRoleRow } from "./Sidebar/OrchestrationPairForm";
import { TeamUnit } from "./Sidebar/TeamUnit";
import type { ModelInfo, ProviderDescriptor, Team } from "../lib/types";

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
  for (const key of [
    "window", "document", "navigator", "HTMLElement", "HTMLSelectElement",
    "SVGElement", "Element", "Node", "Event",
  ] as const) {
    Object.defineProperty(globalThis, key, { value: (window as never)[key], configurable: true });
  }
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

async function openDropdown(trigger: Element): Promise<void> {
  await act(async () => {
    trigger.dispatchEvent(new window.Event("pointerdown", { bubbles: true, cancelable: true }));
  });
}

async function click(element: Element | null | undefined): Promise<void> {
  assert(element, "expected element to click");
  await act(async () => (element as HTMLButtonElement).click());
}

const SEED_MODELS: ModelInfo[] = [
  { id: "m-alpha", display_name: "Alpha One" },
  { id: "m-beta", display_name: "Beta Two", description: "default pick" },
  { id: "m-gamma", display_name: "Gamma Three", available: false },
];

function descriptorWith(models: ModelInfo[]): ProviderDescriptor {
  return {
    schema_version: 1,
    id: "provider-x",
    display_name: "Provider X",
    transport: "pty",
    behavior_config: { model: "own_prompt_set" },
    system_prompt_delivery: "external",
    available_models: models,
    default_model: "m-beta",
    settings_schema: [],
    notifications_schema: [],
    supports_lazy_skills: false,
    has_hooks: false,
    cli_binary: "provider-x",
  } as ProviderDescriptor;
}

function dropdownOptionEntries(testIdPrefix: string): Array<{ id: string; label: string }> {
  return Array.from(document.querySelectorAll(`[data-testid^="${testIdPrefix}-option-"]`))
    .map((option) => ({
      id: option.getAttribute("data-testid")!.replace(`${testIdPrefix}-option-`, ""),
      label: option.textContent?.trim() ?? "",
    }));
}

function expectExactCatalog(entries: Array<{ id: string; label: string }>, models: ModelInfo[], surface: string) {
  assert(
    entries.map((entry) => entry.id).join(",") === models.map((m) => m.id).join(","),
    `${surface}: rendered ids+order must equal the descriptor, got ${entries.map((e) => e.id).join(",")}`,
  );
  for (const [index, model] of models.entries()) {
    assert(
      entries[index].label.includes(model.display_name),
      `${surface}: label for ${model.id} must be "${model.display_name}", got "${entries[index].label}"`,
    );
  }
}

setupDom();

test("New Session single role renders exactly the injected descriptor and follows its mutation", async () => {
  const descriptor = descriptorWith(SEED_MODELS);
  const ui = (models: ModelInfo[]) => (
    <OrchestrationRoleRow
      label="Agent"
      providers={[descriptorWith(models)]}
      providerId="provider-x"
      modelId="m-beta"
      onProviderChange={() => {}}
      onModelChange={() => {}}
      modelTestId="single-model"
    />
  );
  const { host, root } = await render(
    <OrchestrationRoleRow
      label="Agent"
      providers={[descriptor]}
      providerId="provider-x"
      modelId="m-beta"
      onProviderChange={() => {}}
      onModelChange={() => {}}
      modelTestId="single-model"
    />,
  );
  await openDropdown(host.querySelector('[data-testid="single-model"]')!);
  expectExactCatalog(dropdownOptionEntries("single-model"), SEED_MODELS, "single role");

  const mutated: ModelInfo[] = [
    { id: "m-alpha", display_name: "Alpha One Renamed" },
    { id: "m-delta", display_name: "Delta Four" },
  ];
  await act(async () => root.render(ui(mutated)));
  expectExactCatalog(dropdownOptionEntries("single-model"), mutated, "single role after mutation");
  await act(async () => root.unmount());
});

test("New Session pair renders the same descriptor in both roles", async () => {
  const descriptor = descriptorWith(SEED_MODELS);
  const { host, root } = await render(
    <OrchestrationPairForm
      providers={[descriptor]}
      layout="card"
      values={{
        orchestratorProviderId: "provider-x",
        orchestratorModelId: "m-beta",
        plannerProviderId: "provider-x",
        plannerModelId: "m-beta",
      }}
      onChange={() => {}}
      roleTestIds={{ orchestratorModel: "pair-orch-model", plannerModel: "pair-planner-model" }}
    />,
  );
  await openDropdown(host.querySelector('[data-testid="pair-orch-model"]')!);
  expectExactCatalog(dropdownOptionEntries("pair-orch-model"), SEED_MODELS, "pair orchestrator");
  await openDropdown(host.querySelector('[data-testid="pair-planner-model"]')!);
  expectExactCatalog(dropdownOptionEntries("pair-planner-model"), SEED_MODELS, "pair planner");
  await act(async () => root.unmount());
});

test("Team add-member renders exactly the injected descriptor incl. availability and default", async () => {
  const descriptor = descriptorWith(SEED_MODELS);
  const team: Team = { id: "team-1", name: "Team", lifecycle: "active", created_at: 0, workspace_id: "ws" };
  const teamUi = (models: ModelInfo[]) => (
    <TeamUnit
      team={team}
      plan={null}
      tabs={{}}
      managers={null}
      managerCount={1}
      workers={null}
      workerCount={0}
      workingWorkerCount={0}
      providers={[descriptorWith(models)]}
      onOpenEffort={() => {}}
      onAddMember={() => {}}
    />
  );
  const { host, root } = await render(teamUi(SEED_MODELS));

  const openModelStep = async () => {
    await click(host.querySelector('[data-testid="team-menu-trigger"]'));
    await click(
      Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
        .find((button) => button.textContent?.includes("Add worker")),
    );
    await click(
      Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
        .find((button) => button.textContent?.includes(descriptor.display_name)),
    );
  };
  await openModelStep();

  const buttons = Array.from(
    document.querySelectorAll('[data-testid^="team-picker-model-"]'),
  ) as HTMLButtonElement[];
  expectExactCatalog(
    buttons.map((button) => ({
      id: button.getAttribute("data-testid")!.replace("team-picker-model-", ""),
      label: button.textContent?.trim() ?? "",
    })),
    SEED_MODELS,
    "team add-member",
  );
  for (const model of SEED_MODELS) {
    const button = buttons.find((b) => b.getAttribute("data-testid") === `team-picker-model-${model.id}`)!;
    assert(
      button.disabled === (model.available === false),
      `team add-member: ${model.id} disabled state must mirror descriptor availability`,
    );
    assert(
      button.getAttribute("aria-checked") === String(model.id === descriptor.default_model),
      `team add-member: ${model.id} default marker must mirror descriptor default`,
    );
  }

  // Mutation: close, mutate only the descriptor, re-open — no cached second source.
  await click(host.querySelector('[data-testid="team-menu-trigger"]'));
  const mutated: ModelInfo[] = [
    { id: "m-alpha", display_name: "Alpha One Renamed" },
    { id: "m-delta", display_name: "Delta Four", available: false },
  ];
  await act(async () => root.render(teamUi(mutated)));
  await openModelStep();
  const mutatedButtons = Array.from(
    document.querySelectorAll('[data-testid^="team-picker-model-"]'),
  ) as HTMLButtonElement[];
  expectExactCatalog(
    mutatedButtons.map((button) => ({
      id: button.getAttribute("data-testid")!.replace("team-picker-model-", ""),
      label: button.textContent?.trim() ?? "",
    })),
    mutated,
    "team add-member after mutation",
  );
  assert(
    mutatedButtons.find((b) => b.getAttribute("data-testid") === "team-picker-model-m-delta")!.disabled,
    "team add-member after mutation: mutated availability must disable the entry",
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
  console.log(`modelCatalogConsumers tests: ${tests.length - failed}/${tests.length} passed`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
