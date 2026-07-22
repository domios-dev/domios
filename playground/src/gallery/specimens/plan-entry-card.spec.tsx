import { PlanEntryCard } from "../../ds/PlanEntryCard";
import type { Specimen } from "../types";

const base = { title: "General 1.7.2", subtitle: "General 1.7.2 plan", counts: { todo: 1, review: 0, blocked: 0, done: 83 } };

export default {
  id: "plan-entry-card",
  title: "Plan entry card",
  category: "Sidebar",
  where: "src/components/Sidebar/TeamUnit.tsx",
  Component: PlanEntryCard,
  notes: "Shipped truth: TeamUnit.tsx:40-73 maps todo/review/blocked/done and builds contiguous conic segments; TeamUnit.tsx:424-529 renders the 22px donut and status tally; globals.css:1554-1561 and 1842-1847 set its 22px frame and 5px inset hole. The card preserves the liquid progress pop.",
  states: [
    { id: "mixed", label: "Mixed statuses", props: base },
    { id: "blocked", label: "Blocked present", props: { ...base, counts: { todo: 3, review: 1, blocked: 1, done: 79 } } },
    { id: "all-done", label: "All done", props: { ...base, counts: { todo: 0, review: 0, blocked: 0, done: 84 } } },
    { id: "empty", label: "Empty plan", props: { ...base, counts: { todo: 0, review: 0, blocked: 0, done: 0 } } },
    { id: "working", label: "Working", props: { ...base, working: true } },
    { id: "selected", label: "Selected", props: { ...base, selected: true } },
  ],
} satisfies Specimen;
