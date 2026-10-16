import { useEffect, type RefObject } from "react";
import type { ChatSessionIntent } from "@codeterm/chat-core";
import { useSessionGoal } from "./useSessionGoal";
import { goalActions, goalActionLabels, goalConfirmation, goalStatusLabel } from "./goalModel";
import { JobConfirmationDialog } from "../Jobs/JobConfirmationDialog";

export function SessionGoalDetails({ intent, tabId, triggerRef, onUnavailable }: {
  intent: ChatSessionIntent;
  tabId: string;
  triggerRef: RefObject<HTMLButtonElement | null>;
  onUnavailable: () => void;
}) {
  const goal = useSessionGoal(tabId, true, intent, triggerRef);
  const current = goal.snapshot?.supported ? goal.snapshot.intent : intent;
  const unavailable = Boolean(goal.error || goal.snapshot && (!goal.snapshot.supported || !current?.goal?.trim()));
  useEffect(() => {
    if (unavailable) onUnavailable();
  }, [unavailable, onUnavailable]);
  if (unavailable) return null;
  return <div className="min-w-0 whitespace-pre-wrap break-words text-xs">
    {current?.title && <p>{current.title}</p>}
    <p>{current?.goal}</p>
    <p role="status" aria-label="Goal status">{goalStatusLabel(current?.goalStatus)}</p>
    <div role="group" aria-label="Goal actions" className="mt-3 flex flex-wrap gap-2">
      {goalActions(goal.snapshot).map(action => <button type="button" key={action} disabled={goal.busy} onClick={() => goal.select(action)} className="rounded border border-[var(--ct-border)] px-2 py-1">{goalActionLabels[action]}</button>)}
    </div>
    {goal.confirming && <JobConfirmationDialog title={goalActionLabels[goal.confirming.action]} description={`${goalConfirmation(goal.confirming.action)}\n\n${goal.confirming.snapshot.supported ? goal.confirming.snapshot.intent?.goal ?? "" : ""}`} confirmLabel={goalActionLabels[goal.confirming.action]} busy={goal.busy} error={null} onCancel={goal.dismiss} onConfirm={() => void goal.confirm()} />}
  </div>;
}
