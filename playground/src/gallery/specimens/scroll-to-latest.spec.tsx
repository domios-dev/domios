import { ScrollToLatestButton } from "../../../../src/components/Terminal/ScrollToLatestButton";
import { ForcePseudo } from "../stageTools";
import { defineSpecimen } from "../types";

const noop = () => undefined;

function ScrollSpecimen({ compact = false, hover = false }: { compact?: boolean; hover?: boolean }) {
  const button = <div className="gallery-row"><ScrollToLatestButton testId="gallery-scroll-latest" compact={compact} onClick={noop} /></div>;
  return hover ? <ForcePseudo selector=".ct-scroll-latest" state="hover">{button}</ForcePseudo> : button;
}

export default defineSpecimen({
  id: "scroll-to-latest",
  title: "Scroll to latest",
  category: "Composer",
  where: "src/components/Terminal/ScrollToLatestButton.tsx",
  notes: "The frosted chevron that floats over the transcript once the reader scrolls away from the newest message; hover nudges it toward the bottom.",
  Component: ScrollSpecimen,
  states: [
    { id: "normal", label: "Normal", props: {} },
    { id: "hover", label: "Hover", props: { hover: true } },
    { id: "compact", label: "Compact", props: { compact: true } },
  ],
});
