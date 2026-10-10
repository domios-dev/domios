import { DelayedMessageModal, type DelayedModalTarget } from "../../../../src/components/Terminal/DelayedMessageModal";
import type { DelayedMessage } from "../../../../src/components/Terminal/delayedSend";
import { defineSpecimen } from "../types";

const noop = () => undefined;
const text = "Check whether the release build finished and post the summary in the team channel.";

function message(overrides: Partial<DelayedMessage>): DelayedMessage {
  return { id: "delayed-1", tabId: "tab", text, scheduledAtMs: Date.now() + 12 * 60_000, createdAtMs: 1, state: "queued", sessionChanged: false, ...overrides } as DelayedMessage;
}

type ModalSpecimenProps = { target: DelayedModalTarget; busy?: boolean };

function ModalSpecimen({ target, busy = false }: ModalSpecimenProps) {
  const create = target.kind === "create";
  return <div className="gallery-fill" style={{ height: 520 }}>
    <DelayedMessageModal
      target={target}
      busy={busy}
      onSubmit={noop}
      onClose={noop}
      onStash={create ? noop : undefined}
      onDelete={create ? undefined : noop}
      onSendNow={create ? undefined : noop}
    />
  </div>;
}

export default defineSpecimen({
  id: "delayed-message-modal",
  title: "Delayed message modal",
  stage: "wide",
  category: "Composer",
  where: "src/components/Terminal/DelayedMessageModal.tsx",
  notes: "The shipped modal with its scrim, contained in the stage. Handlers are wired the way ChatComposer wires them: create offers Stash; editing offers Delete and Send now; a recurring message edits only its Every interval.",
  Component: ModalSpecimen,
  states: [
    { id: "create", label: "Create · In / At / Every", props: { target: { kind: "create", text } } },
    { id: "edit", label: "Edit · one-off", props: { target: { kind: "edit", message: message({}) } } },
    { id: "recurring", label: "Edit · recurring", props: { target: { kind: "edit", message: message({ text: "Post the morning digest.", recurrence: { everyMs: 60 * 60_000 } }) } } },
    { id: "stashed", label: "Edit · stashed", props: { target: { kind: "edit", message: message({ text: "Draft the changelog once QA signs off", state: "stashed" }) } } },
    { id: "busy", label: "Saving · busy", props: { target: { kind: "edit", message: message({}) }, busy: true } },
  ],
});
