// Run: npx tsx src/lib/newSessionChoice.test.ts

import { Window } from "happy-dom";
import {
  NEW_SESSION_CHOICE_STORAGE_KEY,
  defaultNewSessionChoice,
  readPersistedNewSessionChoice,
  writePersistedNewSessionChoice,
} from "./newSessionChoice";
import type { ModelInfo, ProviderDescriptor } from "./types";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "localStorage", { value: window.localStorage, configurable: true });
}

function provider(id: string, models: ModelInfo[]): ProviderDescriptor {
  return {
    schema_version: 1,
    id,
    display_name: id,
    transport: "pty",
    system_prompt_delivery: "external",
    available_models: models,
    default_model: models[0]?.id,
    settings_schema: [],
    notifications_schema: [],
    supports_lazy_skills: false,
    has_hooks: false,
  } as ProviderDescriptor;
}

const PROVIDERS = [
  provider("provider-a", [
    { id: "model-a", display_name: "Model A" },
    { id: "model-b", display_name: "Model B" },
    { id: "model-c", display_name: "Model C" },
  ]),
  provider("provider-b", [{ id: "model-d", display_name: "Model D" }]),
];

setupDom();

test("default pair derives provider and model only from catalog order", () => {
  const choice = defaultNewSessionChoice(PROVIDERS);
  assert(choice.mode === "pair", "defaults to pair mode");
  if (choice.mode !== "pair") return;
  assert(choice.orchestratorProviderId === PROVIDERS[0].id, "orchestrator uses first catalog provider");
  assert(choice.plannerProviderId === PROVIDERS[0].id, "planner uses first catalog provider");
  assert(choice.orchestratorModelId === PROVIDERS[0].default_model, "orchestrator uses catalog default");
  assert(choice.plannerModelId === PROVIDERS[0].default_model, "planner uses catalog default");
});

test("persistence round-trips the full last choice", () => {
  localStorage.removeItem(NEW_SESSION_CHOICE_STORAGE_KEY);
  const single = { mode: "single" as const, providerId: "provider-b", modelId: "model-d" };
  writePersistedNewSessionChoice(single);
  const readSingle = readPersistedNewSessionChoice(PROVIDERS);
  assert(readSingle.mode === "single", "reads single mode");
  if (readSingle.mode === "single") {
    assert(readSingle.providerId === "provider-b", "provider id round-trips");
    assert(readSingle.modelId === "model-d", "model id round-trips");
  }

  const pair = {
    mode: "pair" as const,
    orchestratorProviderId: "provider-a",
    orchestratorModelId: "model-c",
    plannerProviderId: "provider-b",
    plannerModelId: "model-d",
  };
  writePersistedNewSessionChoice(pair);
  const readPair = readPersistedNewSessionChoice(PROVIDERS);
  assert(readPair.mode === "pair", "reads pair mode");
  if (readPair.mode === "pair") {
    assert(readPair.orchestratorModelId === "model-c", "orchestrator model round-trips");
    assert(readPair.plannerProviderId === "provider-b", "planner provider round-trips");
    assert(readPair.plannerModelId === "model-d", "planner model round-trips");
  }
});

test("persisted choices are re-derived when the plugin catalog changes", () => {
  writePersistedNewSessionChoice({
    mode: "pair",
    orchestratorProviderId: "removed-provider",
    orchestratorModelId: "removed-model",
    plannerProviderId: "provider-a",
    plannerModelId: "removed-model",
  });
  const changedCatalog = [
    provider("provider-a", [{ id: "model-next", display_name: "Model Next" }]),
  ];
  const choice = readPersistedNewSessionChoice(changedCatalog);
  assert(choice.mode === "pair", "pair mode survives catalog refresh");
  if (choice.mode !== "pair") return;
  assert(choice.orchestratorProviderId === "provider-a", "removed provider resolves from current catalog");
  assert(choice.orchestratorModelId === "model-next", "removed orchestrator model resolves from current catalog");
  assert(choice.plannerModelId === "model-next", "removed planner model resolves from current catalog");
});

test("reasoning effort resolves from the provider catalog and round-trips", () => {
  const withEfforts = {
    ...provider("provider-a", [{ id: "model-a", display_name: "Model A" }]),
    reasoning_efforts: [
      { id: "low", display_name: "Low" },
      { id: "high", display_name: "High" },
    ],
    default_reasoning_effort: "high",
  } as ProviderDescriptor;

  const choice = defaultNewSessionChoice([withEfforts]);
  assert(choice.mode === "pair", "defaults to pair mode");
  if (choice.mode !== "pair") return;
  assert(choice.orchestratorEffortId === "high", "defaults to the provider's default effort");

  writePersistedNewSessionChoice({
    mode: "single",
    providerId: "provider-a",
    modelId: "model-a",
    effortId: "low",
  });
  const read = readPersistedNewSessionChoice([withEfforts]);
  assert(read.mode === "single" && read.effortId === "low", "chosen effort round-trips");

  writePersistedNewSessionChoice({
    mode: "single",
    providerId: "provider-a",
    modelId: "model-a",
    effortId: "ultra",
  });
  const reDerived = readPersistedNewSessionChoice([withEfforts]);
  assert(
    reDerived.mode === "single" && reDerived.effortId === "high",
    "an undeclared effort falls back to the provider default",
  );
});

test("per-model efforts narrow persisted choices and use the provider fallback when absent", () => {
  const withEfforts = {
    ...provider("provider-a", [
      {
        id: "wide-model",
        display_name: "Wide Model",
        reasoning_efforts: [
          { id: "low", display_name: "Low" },
          { id: "max", display_name: "Max" },
          { id: "ultra", display_name: "Ultra" },
        ],
      },
      { id: "fallback-model", display_name: "Fallback Model" },
      {
        id: "narrow-model",
        display_name: "Narrow Model",
        reasoning_efforts: [
          { id: "low", display_name: "Low" },
          { id: "high", display_name: "High" },
        ],
      },
    ]),
    reasoning_efforts: [
      { id: "low", display_name: "Low" },
      { id: "medium", display_name: "Medium" },
      { id: "high", display_name: "High" },
    ],
    default_reasoning_effort: "high",
  } as ProviderDescriptor;

  writePersistedNewSessionChoice({
    mode: "single",
    providerId: "provider-a",
    modelId: "narrow-model",
    effortId: "ultra",
  });
  const narrowed = readPersistedNewSessionChoice([withEfforts]);
  assert(narrowed.mode === "single" && narrowed.effortId === "high", "unsupported effort resets to the model's valid default");

  writePersistedNewSessionChoice({
    mode: "single",
    providerId: "provider-a",
    modelId: "fallback-model",
    effortId: "medium",
  });
  const fallback = readPersistedNewSessionChoice([withEfforts]);
  assert(fallback.mode === "single" && fallback.effortId === "medium", "models without per-model levels use provider fallback");
});

test("no effort is selected when the provider declares none", () => {
  const choice = defaultNewSessionChoice(PROVIDERS);
  assert(choice.mode === "pair", "defaults to pair mode");
  if (choice.mode !== "pair") return;
  assert(choice.orchestratorEffortId === undefined, "no declared efforts ⇒ no selection");
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
  console.log(`newSessionChoice tests: ${tests.length - failed}/${tests.length} passed`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
