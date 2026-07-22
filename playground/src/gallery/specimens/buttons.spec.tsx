import { GlassButton } from "../../concepts/liquidglass/GlassButton";
import "../../concepts/liquidglass/liquidglass.css";
import type { Specimen } from "../types";

function ButtonSpecimen({ variant, label }: { variant: "primary" | "accent" | "ghost" | "danger"; label: string }) {
  return <div className="lg-lab"><GlassButton variant={variant}>{label}</GlassButton></div>;
}

export default {
  id: "liquid-buttons", title: "Glass buttons", category: "Buttons", where: "concepts/liquidglass/GlassButton.tsx", Component: ButtonSpecimen,
  notes: "Press any stage to focus it, then press the button to inspect its release pop.",
  states: [
    { id: "primary", label: "Primary", props: { variant: "primary", label: "Run agent" } },
    { id: "accent", label: "Accent", props: { variant: "accent", label: "Working" } },
    { id: "ghost", label: "Ghost", props: { variant: "ghost", label: "Details" } },
    { id: "danger", label: "Danger", props: { variant: "danger", label: "Stop" } },
  ],
} satisfies Specimen;
