import { useState } from "react";
import { LiquidFlip, LiquidToggle } from "../../concepts/liquidglass/LiquidControls";
import "../../concepts/liquidglass/liquidglass.css";
import type { Specimen } from "../types";

function ToggleDemo({ kind, initial }: { kind: "toggle" | "flip"; initial: boolean }) {
  const [checked, setChecked] = useState(initial);
  return <div className="lg-lab">{kind === "toggle" ? <LiquidToggle checked={checked} onChange={setChecked} ariaLabel="Live tail" /> : <LiquidFlip checked={checked} onChange={setChecked} ariaLabel="Autopilot" />}</div>;
}

export default {
  id: "liquid-toggles", title: "Liquid toggles", category: "Controls", where: "concepts/liquidglass/LiquidControls.tsx", Component: ToggleDemo,
  states: [
    { id: "toggle-off", label: "Toggle · off", props: { kind: "toggle", initial: false } },
    { id: "toggle-on", label: "Toggle · on", props: { kind: "toggle", initial: true } },
    { id: "flip-off", label: "Flip · off", props: { kind: "flip", initial: false } },
    { id: "flip-on", label: "Flip · on", props: { kind: "flip", initial: true } },
  ],
} satisfies Specimen;
