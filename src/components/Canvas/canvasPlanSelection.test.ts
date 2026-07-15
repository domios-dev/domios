import { selectCanvasPlans } from "./canvasPlanSelection";
import { FIXTURE_PLAN, FIXTURE_TASK } from "./cards/fixtures.testdata";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const live = [{ ...FIXTURE_PLAN, id: "team-plan", tasks: [] }];
const fixture = [{ ...FIXTURE_PLAN, id: "plan-perf-plan", tasks: [{ ...FIXTURE_TASK, id: "synthetic" }] }];

const teamSelection = selectCanvasPlans(live, fixture, "team-plan");
assert(teamSelection === live || teamSelection[0]?.id === "team-plan", "Team target selects live plans");
assert(!teamSelection.some((plan) => plan.id === "plan-perf-plan"), "Team target excludes unrelated fixtures");

const fixtureSelection = selectCanvasPlans(live, fixture);
assert(fixtureSelection[0]?.id === "plan-perf-plan", "untargeted performance mode keeps fixtures");

console.log("canvas plan selection tests passed");
