import { BackPill } from "../../../../src/components/shared/BackPill";
import { ForcePseudo, type PseudoState } from "../stageTools";
import { defineSpecimen } from "../types";

function BackSpecimen({ label, ariaLabel, state }: { label?: string; ariaLabel?: string; state?: PseudoState }) {
  const pill = <BackPill label={label} ariaLabel={ariaLabel} onBack={() => undefined} />;
  return state ? <ForcePseudo selector=".ct-bar-pill" state={state}><div className="gallery-row">{pill}</div></ForcePseudo> : pill;
}

export default defineSpecimen({
  id: "back-pill",
  title: "Back pill",
  category: "Buttons",
  where: "src/components/shared/BackPill.tsx",
  notes: "The one way back out of a view, on the shared ct-bar-pill. The visible word is always Back; the accessible name and tooltip say where it returns (Back to Settings) or what it does (Close settings).",
  Component: BackSpecimen,
  states: [
    { id: "plain", label: "Back", props: {} },
    { id: "to-section", label: "Back to Settings · named target", props: { label: "Settings" } },
    { id: "close", label: "Close settings · aria override", props: { ariaLabel: "Close settings" } },
    { id: "hover", label: "Hover", props: { label: "Settings", state: "hover" } },
    { id: "focus", label: "Keyboard focus", props: { label: "Settings", state: "focus-visible" } },
  ],
});
