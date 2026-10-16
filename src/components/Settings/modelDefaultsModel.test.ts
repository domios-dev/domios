import assert from "node:assert/strict";
import { test } from "node:test";
import type { AppConfig } from "../../lib/types";
import { modelDefaultGuideSection, modelDefaultPatch, modelDefaultRows } from "./modelDefaultsModel";

function config(overrides: Partial<AppConfig>): AppConfig {
  return {
    agents: {},
    general_agent: { agent_provider: "codex" },
    memory: { extraction_agent: "", extraction_model: "", dreaming_agent: "", dreaming_model: "", worker_model: "" },
    ...overrides,
  } as unknown as AppConfig;
}

test("editing a default preserves other provider and memory settings", () => {
  const original = config({ agents: { fixture: { default_model: "old", visible_in_menu: false } } });
  const row = modelDefaultRows(original)[0];
  assert.deepEqual(modelDefaultPatch(original, row, "next"), {
    agents: { fixture: { default_model: "next", visible_in_menu: false } },
  });
  const memoryRow = { key: "memory.extraction_model", subject: "opaque", provider: "fixture", model: "old" };
  assert.deepEqual(modelDefaultPatch(original, memoryRow, "next"), {
    memory: { ...original.memory, extraction_model: "next" },
  });
});

test("every stored default is listed with the provider that owns it", () => {
  const rows = modelDefaultRows(config({
    agents: { opencode: { default_model: "x/y" }, codex: { default_model: "gpt-6-sol" }, claude: { default_model: " " } },
    memory: { extraction_agent: "claude", extraction_model: "haiku", dreaming_agent: "", dreaming_model: "", worker_model: "gpt-5.5" } as AppConfig["memory"],
  }));
  assert.deepEqual(rows.map((row) => [row.key, row.provider, row.model]), [
    ["agents.codex.default_model", "codex", "gpt-6-sol"],
    ["agents.opencode.default_model", "opencode", "x/y"],
    ["memory.extraction_model", "claude", "haiku"],
    ["memory.worker_model", "codex", "gpt-5.5"],
  ]);
});

test("a replacement note shows only while the replaced value is still configured", () => {
  const replaced = { "agents.codex.default_model": { provider: "codex", from: "gpt-5.1-codex", to: "gpt-6-sol", reason: "r" } };
  const current = modelDefaultRows(config({ agents: { codex: { default_model: "GPT-6-SOL" } }, model_defaults: { replaced } }));
  assert.equal(current[0].replacedFrom, "gpt-5.1-codex");
  const changedSince = modelDefaultRows(config({ agents: { codex: { default_model: "gpt-5.5" } }, model_defaults: { replaced } }));
  assert.equal(changedSince[0].replacedFrom, undefined);
});

test("model default help follows its setting namespace", () => {
  const row = { key: "memory.extraction_model", subject: "opaque", provider: "fixture", model: "m" };
  assert.equal(modelDefaultGuideSection(row), "memory");
  assert.equal(modelDefaultGuideSection({ ...row, key: "agents.fixture.default_model" }), "agents");
});


test("provider tier defaults persist explicit on and off alongside model defaults", () => {
  const original = config({ agents: { arbitrary: { default_model: "old", visible_in_menu: false } } });
  const row = modelDefaultRows(original)[0];
  for (const fastMode of [true, false]) {
    assert.deepEqual(modelDefaultPatch(original, row, "next", fastMode).agents?.arbitrary,
      { default_model: "next", visible_in_menu: false, default_fast_mode: fastMode });
  }
});
