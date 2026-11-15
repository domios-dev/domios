import { describeError } from "../../lib/describeError";
import { InlineErrorNotice } from "../shared/ErrorNotice";

export function ChecklistReadFailure({ error, onRetry }: { error: unknown; onRetry: () => void }) {
  return <div className="sidebar-agent-checklist__note">
    <InlineErrorNotice description={{ ...describeError(error), title: "Could not read the saved checklist.", cause: "" }}
      actions={[{ label: "Retry checklist", onClick: onRetry }]} />
  </div>;
}
