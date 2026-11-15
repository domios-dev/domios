import { describeError } from "../../lib/describeError";
import { InlineErrorNotice } from "../shared/ErrorNotice";

export function ChecklistReadFailure({ error, onRetry }: { error: unknown; onRetry: () => void }) {
  return <div className="sidebar-agent-checklist__note [&_p]:font-normal [&_button]:border-0 [&_button]:p-0 [&_button]:text-[var(--ct-accent)] [&_button:hover]:underline">
    <InlineErrorNotice description={{ ...describeError(error), title: "Could not read the saved checklist.", cause: "" }}
      actions={[{ label: "Retry checklist", onClick: onRetry }]} />
  </div>;
}
