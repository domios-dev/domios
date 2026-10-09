import { PlanStatusBreakdown, type PlanStatusCounts } from "../../../../src/components/shared/PlanStatusBreakdown";
import { defineSpecimen } from "../types";

const counts = (overrides: Partial<PlanStatusCounts>): PlanStatusCounts => ({ todo: 0, review: 0, blocked: 0, paused: 0, done: 0, ...overrides });

export default defineSpecimen({
  id: "plan-status-breakdown",
  title: "Plan status breakdown",
  category: "Status",
  where: "src/components/shared/PlanStatusBreakdown.tsx",
  Component: ({ counts: value }: { counts: PlanStatusCounts }) => <PlanStatusBreakdown counts={value} />,
  states: [
    { id: "mixed", label: "Mixed", props: { counts: counts({ todo: 1, review: 2, blocked: 1, done: 83 }) } },
    { id: "done", label: "All done", props: { counts: counts({ done: 84 }) } },
    { id: "empty", label: "Empty", props: { counts: counts({}) } },
  ],
});
