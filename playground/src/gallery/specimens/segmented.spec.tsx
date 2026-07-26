import { useState } from "react";
import { LiquidSegmented } from "../../concepts/liquidglass/LiquidControls";
import "../../concepts/liquidglass/liquidglass.css";
import { defineSpecimen } from "../types";

function SegmentedDemo({ options, initial }: { options: string[]; initial: number }) {
  const [value, setValue] = useState(initial);
  return <div className="lg-lab"><LiquidSegmented options={options} value={value} onChange={setValue} ariaLabel={`${options.length} option segmented control`} /></div>;
}

export default defineSpecimen({
  id: "liquid-segmented", title: "Liquid segmented", category: "Controls", where: "concepts/liquidglass/LiquidControls.tsx", Component: SegmentedDemo,
  states: [
    { id: "two-options", label: "Two options", props: { options: ["Local", "Mesh"], initial: 0 } },
    { id: "three-options", label: "Three options", props: { options: ["Compact", "Cozy", "Roomy"], initial: 1 } },
  ],
});
