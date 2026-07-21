import { LivingDots } from "../../concepts/liquidglass/LivingDots";
import "../../concepts/liquidglass/liquidglass.css";
import type { Specimen } from "../types";

function LivingDotsSpecimen(props: Record<string, unknown>) {
  return <div className="lg-lab"><LivingDots {...props} /></div>;
}

export default {
  id: "living-dots",
  title: "Living Dots",
  where: "liquid glass · agent status",
  Component: LivingDotsSpecimen,
  notes: "The three-beat squash-jump signature used for working, thinking, and idle agent states.",
  states: [
    { id: "working", label: "Working", props: { variant: "signature", label: "working" } },
    { id: "thinking", label: "Thinking", props: { variant: "thinking", label: "thinking" } },
    { id: "idle", label: "Idle", props: { variant: "signature", idle: true, label: "idle" } },
  ],
} satisfies Specimen;
