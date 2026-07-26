import { LivingDots } from "../../ds/LivingDots";
import { defineSpecimen } from "../types";

function LivingDotsSpecimen(props: Record<string, unknown>) {
  return <div className="lg-lab"><LivingDots {...props} /></div>;
}

export default defineSpecimen({
  id: "living-dots",
  title: "Living Dots",
  category: "Status",
  where: "shared DS primitive · agent status",
  Component: LivingDotsSpecimen,
  notes: "The shared three-beat squash-jump signature for all live and idle agent states.",
  states: [
    { id: "accent-medium", label: "Accent · medium", props: { tone: "accent", size: "medium" } },
    { id: "thinking-small", label: "Thinking · small", props: { tone: "thinking", size: "small", rhythm: "thinking" } },
    { id: "muted-micro", label: "Muted · micro", props: { tone: "muted", size: "micro" } },
    { id: "idle", label: "Idle", props: { tone: "accent", size: "small", idle: true } },
  ],
});
