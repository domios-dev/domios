import assert from "node:assert/strict";
import { teamCloseOutcome, startBackgroundClose, teamCloseSurvivors } from "./teamCloseOutcome";
import type { Tab, CronJob } from "../../lib/types";
import type { BlueprintSnapshot } from "../Automation/types";

const empty = { removed: [], kept: [], orphan_branches: [] };
assert.deepEqual(teamCloseOutcome(empty), { kind: "closed", survivors: [], preserved: [] });
const kept = { ...empty, kept: [{ path: "C:/lanes/work", branch: "lane/work", reason: "dirty" }] };
assert.deepEqual(teamCloseOutcome(kept), { kind: "closed_with_preserved_lanes", survivors: [], preserved: ["C:/lanes/work"] });
assert.deepEqual(teamCloseOutcome(kept, ["Worker"]), { kind: "partial", survivors: ["Worker"], preserved: ["C:/lanes/work"] });
assert.deepEqual(teamCloseSurvivors("team", [
  { id: "worker", title: "Worker", team_id: "team" } as Tab,
  { id: "foreign", title: "Foreign", team_id: "other" } as Tab,
], [
  { id: "cron", name: "Cadence", team_id: "team" } as CronJob,
  { id: "other-cron", name: "Other cadence", team_id: "other" } as CronJob,
], [
  { name: "Team graph", state: "running", owner: { kind: "team", team_id: "team" } } as BlueprintSnapshot,
  { name: "Worker graph", state: "running", owner: { kind: "tab", tab_id: "worker" } } as BlueprintSnapshot,
  { name: "Other graph", state: "running", owner: { kind: "tab", tab_id: "foreign" } } as BlueprintSnapshot,
  { name: "Stopped graph", state: "idle", owner: { kind: "team", team_id: "team" } } as BlueprintSnapshot,
]), ["Tab: Worker", "Cron: Cadence", "Blueprint: Team graph", "Blueprint: Worker graph"]);
let dismissed = false;
void startBackgroundClose(() => { dismissed = true; }, () => {
  assert.equal(dismissed, true);
  return Promise.resolve(empty);
});
