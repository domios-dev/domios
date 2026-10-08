// Run: npx tsx packages/chat-ui/src/blocks/connectSettingModels.test.ts

import { connectResult, connectStatus, needsTokenEntry, parseBridgeId, type BridgeConnectInfo } from "./connectBlockModel";
import { canStep, choiceLayout, isSensitiveKey, parseSettingKey, stepValue } from "./settingBlockModel";

const tests: Array<[string, () => void]> = [];
function test(name: string, run: () => void) {
  tests.push([name, run]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function bridge(overrides: Partial<BridgeConnectInfo> = {}): BridgeConnectInfo {
  return {
    id: "telegram", name: "Telegram", description: "d", phase: "disconnected", tokenStored: false,
    method: { kind: "token", label: "Bot token", placeholder: "p", help: "h" },
    ...overrides,
  };
}

test("bridge_ids_are_validated", () => {
  assert(parseBridgeId("telegram") === "telegram" && parseBridgeId(" teams ") === "teams", "valid id rejected");
  for (const id of ["Telegram", "1tg", "tele gram", "../x", "", "a".repeat(40)]) assert(parseBridgeId(id) === null, `accepted ${id}`);
});

test("each_phase_has_a_distinct_status_tone", () => {
  const tones = (["connected", "connecting", "failing", "disconnected"] as const).map((phase) => connectStatus(phase).tone);
  assert(new Set(tones).size === 4 && tones[0] === "ok" && tones[3] === "muted", `tones ${tones.join(",")}`);
});

test("token_entry_shows_without_a_saved_token_or_when_replacing_and_never_once_connected", () => {
  assert(needsTokenEntry(bridge(), false), "missing token must ask for one");
  assert(!needsTokenEntry(bridge({ tokenStored: true }), false), "saved token must connect without entry");
  assert(needsTokenEntry(bridge({ tokenStored: true }), true), "replace must show the field");
  assert(!needsTokenEntry(bridge({ phase: "connected" }), true), "connected bridge never asks for a token");
});

test("connect_result_is_judged_against_the_state_after_the_click", () => {
  assert(connectResult({ kind: "ready", bridge: bridge({ phase: "connected", identity: "bot 42" }) }) === "Connected as bot 42.", "identity lost");
  assert(connectResult({ kind: "ready", bridge: bridge({ phase: "failing", error: "401" }) }).includes("401"), "failure reason lost");
  assert(connectResult({ kind: "loading" }).includes("unknown"), "unknown state must not claim success");
});

test("setting_keys_are_dot_paths_without_traversal", () => {
  for (const key of ["plugins.chat-preprocessor.enabled", "agents.claude.resume_sessions", "plugins.x.memK"]) {
    assert(parseSettingKey(key) === key, `rejected ${key}`);
  }
  for (const key of ["enabled", "plugins..x", ".a.b", "a.b.", "a b.c", "a.b/c", "plugins.x.y z"]) {
    assert(parseSettingKey(key) === null, `accepted ${key}`);
  }
});

test("secret_looking_keys_are_sensitive_in_any_segment", () => {
  for (const key of ["api.token", "plugins.x.apiKey", "plugins.x.api_key", "integrations.secrets.backend", "plugins.x.password", "plugins.bot_token.enabled", "agents.x.OPENAI_API_KEY"]) {
    assert(isSensitiveKey(key), `${key} not sensitive`);
  }
  for (const key of ["plugins.chat-preprocessor.enabled", "notifications.desktop", "agents.claude.model"]) {
    assert(!isSensitiveKey(key), `${key} wrongly sensitive`);
  }
});

test("short_option_sets_are_segmented_and_long_ones_a_select", () => {
  assert(choiceLayout([{ value: "a", label: "Before" }, { value: "b", label: "After" }]) === "segmented", "short options not segmented");
  assert(choiceLayout(["1", "2", "3", "4", "5"].map((value) => ({ value, label: value }))) === "select", "five options not a select");
  assert(choiceLayout([{ value: "a", label: "A very long option label" }]) === "select", "long label not a select");
});

test("stepper_clamps_to_the_schema_range", () => {
  const control = { kind: "stepper" as const, min: 1, max: 8, step: 1 };
  assert(stepValue(3, 1, control) === 4 && stepValue(8, 1, control) === 8 && stepValue(1, -1, control) === 1, "range not respected");
  assert(!canStep(8, 1, control) && canStep(8, -1, control), "canStep wrong at the edge");
  assert(stepValue(0.3, 1, { kind: "stepper", step: 0.1 }) === 0.4, "fractional step drifted");
});

let failed = 0;
for (const [name, run] of tests) {
  try {
    run();
    console.log(`  ✓ ${name}`);
  } catch (error) {
    failed++;
    console.error(`  ✗ ${name}: ${error instanceof Error ? error.message : error}`);
  }
}
console.log(`chat-ui/blocks/connectSettingModels tests: ${failed === 0 ? "all passed" : `${failed} failed`}`);
process.exit(failed === 0 ? 0 : 1);
