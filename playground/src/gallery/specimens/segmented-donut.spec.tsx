import { SegmentedDonut } from "../../ds/SegmentedDonut";
import { defineSpecimen } from "../types";

export default defineSpecimen({
  id: "segmented-donut",
  title: "Segmented donut",
  category: "Status",
  where: "shared DS primitive · plan status composition",
  Component: SegmentedDonut,
  notes: "Status category: this reusable visualizes composition independently of the sidebar card that consumes it.",
  states: [
    { id: "mixed", label: "Mixed", props: { size: 44, segments: [{ status: "todo", count: 3 }, { status: "review", count: 1 }, { status: "blocked", count: 1 }, { status: "done", count: 3 }] } },
    { id: "complete", label: "Complete", props: { size: 44, segments: [{ status: "done", count: 8 }] } },
    { id: "empty", label: "Empty", props: { size: 44, segments: [] } },
  ],
});
