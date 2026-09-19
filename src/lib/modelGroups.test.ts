// Run: npx tsx src/lib/modelGroups.test.ts

import {
  formatModelCost,
  formatModelUsage,
  hasModelUsage,
  modelGroupForId,
  modelIdPrefix,
  usageForModel,
} from "./modelGroups";
import type { ModelGroup, ModelGroupUsage } from "./types";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const GROUPS: ModelGroup[] = [
  { id: "deepseek", label: "DeepSeek" },
  { id: "xai", label: "xAI" },
  { id: "opencode", label: "OpenCode Zen" },
];

function usage(group: string, overrides: Partial<ModelGroupUsage> = {}): ModelGroupUsage {
  return {
    group,
    messages: 0,
    input_tokens: 0,
    output_tokens: 0,
    cache_read_tokens: 0,
    cache_write_tokens: 0,
    cost_micros: 0,
    ...overrides,
  };
}

test("model id prefix is the segment before the first slash", () => {
  assert(modelIdPrefix("deepseek/deepseek-v4-flash") === "deepseek", "prefix extracted");
  assert(modelIdPrefix("bare-model") === "bare-model", "no slash ⇒ whole id");
});

test("a model resolves to the declared group that prefixes it", () => {
  const group = modelGroupForId(GROUPS, "xai/grok-4.6");
  assert(group?.label === "xAI", `expected xAI, got ${group?.label}`);
});

test("the longest matching group wins when prefixes nest", () => {
  const nested: ModelGroup[] = [
    { id: "open", label: "Open" },
    { id: "open/router", label: "Open Router" },
  ];
  const group = modelGroupForId(nested, "open/router/gpt");
  assert(group?.label === "Open Router", `expected the longest match, got ${group?.label}`);
});

test("an undeclared prefix resolves to no group", () => {
  assert(modelGroupForId(GROUPS, "anthropic/claude-opus") === null, "no group");
});

test("usage is matched by declared group id", () => {
  const usages = [usage("xai", { cost_micros: 1_500_000, input_tokens: 10 })];
  const found = usageForModel(usages, GROUPS, "xai/grok-4.6");
  assert(found?.cost_micros === 1_500_000, "usage resolved for the group");
});

test("usage falls back to the bare id prefix when no group is declared", () => {
  const usages = [usage("deepseek", { cost_micros: 2_000_000 })];
  const found = usageForModel(usages, undefined, "deepseek/deepseek-v4-flash");
  assert(found?.cost_micros === 2_000_000, "prefix fallback resolves usage");
});

test("a group with no usage entry resolves to null, never a guess", () => {
  assert(usageForModel([], GROUPS, "xai/grok-4.6") === null, "no usage ⇒ null");
});

test("zeroed usage is not shown", () => {
  assert(!hasModelUsage(usage("xai")), "all-zero usage renders nothing");
  assert(!hasModelUsage(null), "null usage renders nothing");
  assert(hasModelUsage(usage("xai", { messages: 1 })), "a single message is shown");
  assert(hasModelUsage(usage("xai", { cost_micros: 1 })), "a nonzero cost is shown");
});

test("cost and token formatting stay compact", () => {
  assert(formatModelCost(1_942_500) === "$1.94", "dollars with cents");
  assert(formatModelCost(1) === "<$0.01", "sub-cent cost is explicit");
  assert(formatModelCost(0) === "$0.00", "zero cost");
  assert(
    formatModelUsage(usage("xai", { cost_micros: 1_940_000, input_tokens: 4_800_000 })) ===
      "$1.94 · 4.8M tok",
    "usage label combines cost and tokens",
  );
});

async function main() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`ok ${name}`);
    } catch (error) {
      failed += 1;
      console.error(`FAIL ${name}`);
      console.error(error);
    }
  }
  console.log(`modelGroups tests: ${tests.length - failed}/${tests.length} passed`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
