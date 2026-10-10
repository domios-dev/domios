import { strict as assert } from "node:assert";
import { test } from "node:test";
import { fixtureNode } from "../Automation/fixtures.testdata";
import { blueprintTitle, type BlueprintScenarioJob } from "../Automation/scenarios";
import type { AutomationIdentity } from "../Automation/types";
import { sentinelRowText } from "./JobRow";

const root: AutomationIdentity = { kind: "blueprint", id: "sentinel_7f3a9c", owner_tab_id: null, workspace_id: "ws", team_id: null };

function job(overrides: Partial<ReturnType<typeof fixtureNode>> = {}, name = "scenario_7f3a9c"): BlueprintScenarioJob {
  return { scenario: { id: "scenario_7f3a9c", name, root }, type: "Blueprint", root: { ...fixtureNode(root, root.id), ...overrides }, state: "active" };
}

function visible(row: BlueprintScenarioJob): string {
  const title = blueprintTitle(row.scenario, row.root);
  return `${title} ${sentinelRowText(row, title).meta}`;
}

test("an unnamed Blueprint never shows its graph or node id as row text", () => {
  for (const row of [job({ purpose: null }), job({ purpose: null, schedule: "*/5 * * * *" }), job({ purpose: null, cadence_ms: 90_000 }), job({ purpose: "Watch deploys" })]) {
    assert.ok(!visible(row).includes("7f3a9c"), visible(row));
  }
});

test("the graph id stays reachable as a hover hint", () => {
  const row = job({ purpose: null });
  assert.ok(sentinelRowText(row, blueprintTitle(row.scenario, row.root)).idHint.includes(row.scenario.id));
});

test("the secondary line does not repeat a purpose already used as the title", () => {
  const row = job({ purpose: "Inspect production health", schedule: "0 * * * *" });
  const title = blueprintTitle(row.scenario, row.root);
  assert.equal(title, "Inspect production health");
  assert.notEqual(sentinelRowText(row, title).meta, title);
});

test("a named Blueprint keeps its purpose as the secondary line", () => {
  const row = job({ purpose: "Watch deploys" }, "Deploy watcher");
  assert.equal(sentinelRowText(row, blueprintTitle(row.scenario, row.root)).meta, "Watch deploys");
});
