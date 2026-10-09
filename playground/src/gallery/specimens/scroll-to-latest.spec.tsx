import { ScrollToLatestButton } from "../../ds/ScrollToLatestButton";
import { defineSpecimen } from "../types";

export default defineSpecimen({
  id: "scroll-to-latest",
  title: "Scroll to latest",
  category: "Composer",
  where: "src/components/Terminal/ScrollToLatestButton.tsx",
  notes: "A frosted round chevron that floats over the transcript when the reader has scrolled away from the newest message; hover nudges it toward the bottom. The compact size serves dense panes.",
  states: [
    { id: "normal", label: "Normal", props: {} },
    { id: "compact", label: "Compact", props: { compact: true } },
  ],
  Component: ScrollToLatestButton,
});
