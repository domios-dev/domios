import assert from "node:assert/strict";
import { test } from "node:test";
import type { AppConfig } from "../../lib/types";
import { modelDefaultRows } from "./modelDefaultsModel";

function config(overrides: Partial<AppConfig>): AppConfig {
  return {
    agents: {},
    general_agent: { agent_provider: "codex" },
    memory: { extraction_agent: "", extraction_model: "", dreaming_agent: "", dreaming_model: "", worker_model: "" },
    ...overrides,
  } as unknown as AppConfig;
}

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
