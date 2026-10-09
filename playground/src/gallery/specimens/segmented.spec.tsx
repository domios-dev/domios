import { useState } from "react";
import { LiquidTabRail } from "../../../../src/components/shared/LiquidTabRail";
import { Segmented } from "../../../../src/components/shared/Segmented";
import { defineSpecimen } from "../types";

function SegmentedSpecimen({ kind, options, initial }: { kind: "segmented" | "rail"; options: string[]; initial: number }) {
  const [value, setValue] = useState(options[initial]);
  if (kind === "rail") return <LiquidTabRail value={value} options={options} onChange={setValue} label="Gallery rail" />;
  return <Segmented value={value} options={options.map((option) => ({ value: option, label: option }))} onChange={setValue} aria-label={`${options.length} option segmented control`} />;
}

export default defineSpecimen({
  id: "liquid-segmented",
  title: "Segmented controls",
  category: "Controls",
  where: "src/components/shared/Segmented.tsx · LiquidTabRail.tsx",
  notes: "Click a segment to watch the shipped liquid thumb travel.",
  Component: SegmentedSpecimen,
  states: [
    { id: "two-options", label: "Segmented · two options", props: { kind: "segmented", options: ["Local", "Mesh"], initial: 0 } },
    { id: "three-options", label: "Segmented · three options", props: { kind: "segmented", options: ["Compact", "Cozy", "Roomy"], initial: 1 } },
    { id: "rail", label: "Liquid tab rail", props: { kind: "rail", options: ["Board", "Plan", "Docs", "Changes"], initial: 0 } },
  ],
});
