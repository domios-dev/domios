// Run: npx tsx src/lib/newSessionChoice.test.ts

import { Window } from "happy-dom";
import {
  NEW_SESSION_CHOICE_STORAGE_KEY,
  defaultNewSessionChoice,
  readPersistedNewSessionChoice,
  writePersistedNewSessionChoice,
} from "./newSessionChoice";
import type { ProviderDescriptor } from "./types";

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

function provider(id: string, models: Array<{ id: string; display_name: string }>): ProviderDescriptor {
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
  provider("claude", [
    { id: "opus", display_name: "Opus" },
    { id: "fable", display_name: "Fable" },
    { id: "sonnet", display_name: "Sonnet" },
  ]),
  provider("codex", [{ id: "gpt-5", display_name: "GPT-5" }]),
];

setupDom();

test("default pair is claude opus + claude fable", () => {
  const choice = defaultNewSessionChoice(PROVIDERS);
  assert(choice.mode === "pair", "defaults to pair mode");
  if (choice.mode !== "pair") return;
  assert(choice.orchestratorProviderId === "claude", "orchestrator provider is claude");
  assert(choice.plannerProviderId === "claude", "planner provider is claude");
  assert(choice.orchestratorModelId === "opus", `orchestrator model opus, got ${choice.orchestratorModelId}`);
  assert(choice.plannerModelId === "fable", `planner model fable, got ${choice.plannerModelId}`);
});

test("persistence round-trips the full last choice", () => {
  localStorage.removeItem(NEW_SESSION_CHOICE_STORAGE_KEY);
  const single = { mode: "single" as const, providerId: "codex", modelId: "gpt-5" };
  writePersistedNewSessionChoice(single);
  const readSingle = readPersistedNewSessionChoice(PROVIDERS);
  assert(readSingle.mode === "single", "reads single mode");
  if (readSingle.mode === "single") {
    assert(readSingle.providerId === "codex", "provider id round-trips");
    assert(readSingle.modelId === "gpt-5", "model id round-trips");
  }

  const pair = {
    mode: "pair" as const,
    orchestratorProviderId: "claude",
    orchestratorModelId: "sonnet",
    plannerProviderId: "codex",
    plannerModelId: "gpt-5",
  };
  writePersistedNewSessionChoice(pair);
  const readPair = readPersistedNewSessionChoice(PROVIDERS);
  assert(readPair.mode === "pair", "reads pair mode");
  if (readPair.mode === "pair") {
    assert(readPair.orchestratorModelId === "sonnet", "orchestrator model round-trips");
    assert(readPair.plannerProviderId === "codex", "planner provider round-trips");
    assert(readPair.plannerModelId === "gpt-5", "planner model round-trips");
  }
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
