import { SegmentedDonut, type SegmentedDonutSegment } from "../../../../src/components/shared/SegmentedDonut";
import { defineSpecimen } from "../types";

function DonutSpecimen({ segments }: { segments: SegmentedDonutSegment[] }) {
  return <div className="gallery-row">
    <SegmentedDonut segments={segments} size={22} />
    <SegmentedDonut segments={segments} size={44} />
  </div>;
}

export default defineSpecimen({
  id: "segmented-donut",
  title: "Segmented donut",
  category: "Status",
  where: "src/components/shared/SegmentedDonut.tsx",
  notes: "Plan status composition at the shipped 22px sidebar size and enlarged. The Team unit specimen shows it inside the plan entry card.",
  Component: DonutSpecimen,
  states: [
    { id: "mixed", label: "Mixed", props: { segments: [{ status: "todo", count: 3 }, { status: "review", count: 1 }, { status: "blocked", count: 1 }, { status: "done", count: 3 }] } },
    { id: "paused", label: "With paused", props: { segments: [{ status: "todo", count: 2 }, { status: "paused", count: 2 }, { status: "done", count: 4 }] } },
    { id: "complete", label: "Complete", props: { segments: [{ status: "done", count: 8 }] } },
    { id: "empty", label: "Empty", props: { segments: [] } },
  ],
});
