import { DelayedMessageModal } from "../../ds/DelayedMessageModal";
import { defineSpecimen } from "../types";

const text = "Check whether the release build finished and post the summary in the team channel.";

export default defineSpecimen({
  id: "delayed-message-modal",
  title: "Delayed message modal",
  stage: "wide",
  category: "Composer",
  where: "src/components/Terminal/DelayedMessageModal.tsx",
  notes: "Rendered inline without the portal scrim. Creating offers In, At and Every with quick presets plus Stash; editing a one-off keeps In and At with Delete and Send now; a recurring message edits only its Every interval.",
  states: [
    { id: "create", label: "Create", props: { variant: "create", text } },
    { id: "edit", label: "Edit · one-off", props: { variant: "edit", text, minutes: 12 } },
    { id: "recurring", label: "Edit · recurring", props: { variant: "recurring", text: "Post the morning digest.", minutes: 60 } },
  ],
  Component: DelayedMessageModal,
});
