import { useState } from "react";
import { LiquidFlip, LiquidToggle } from "../../../../src/components/shared/LiquidToggle";
import { defineSpecimen } from "../types";

function ToggleSpecimen({ kind, initial, disabled = false }: { kind: "toggle" | "flip"; initial: boolean; disabled?: boolean }) {
  const [checked, setChecked] = useState(initial);
  const Control = kind === "toggle" ? LiquidToggle : LiquidFlip;
  return <Control checked={checked} onCheckedChange={setChecked} disabled={disabled} aria-label={kind === "toggle" ? "Live tail" : "Autopilot"} />;
}

export default defineSpecimen({
  id: "liquid-toggles",
  title: "Liquid toggles",
  category: "Controls",
  where: "src/components/shared/LiquidToggle.tsx",
  notes: "Click to watch the shipped liquid travel between states.",
  Component: ToggleSpecimen,
  states: [
    { id: "toggle-off", label: "Toggle · off", props: { kind: "toggle", initial: false } },
    { id: "toggle-on", label: "Toggle · on", props: { kind: "toggle", initial: true } },
    { id: "toggle-disabled", label: "Toggle · disabled", props: { kind: "toggle", initial: true, disabled: true } },
    { id: "flip-off", label: "Flip · off", props: { kind: "flip", initial: false } },
    { id: "flip-on", label: "Flip · on", props: { kind: "flip", initial: true } },
  ],
});
