import {Target} from "lucide-react";
import type {ChatSessionIntent} from "@codeterm/chat-core";
import {ComposerContextChip} from "./ComposerContextChip";
import {useRuntimeStatusPopover} from "./useRuntimeStatusPopover";
import {useSessionGoal} from "./useSessionGoal";
import {goalActions,goalActionLabels,goalConfirmation,goalStatusLabel} from "./goalModel";
import {JobConfirmationDialog} from "../Jobs/JobConfirmationDialog";
import {InlineErrorNotice} from "../shared/ErrorNotice";
export function SessionGoalBubble({intent,tabId}:{intent:ChatSessionIntent;tabId:string}) {
  const {open,triggerRef,popoverRef,toggle}=useRuntimeStatusPopover();
  const goal=useSessionGoal(tabId,open,intent,triggerRef);
  const current=goal.snapshot?.supported ? goal.snapshot.intent : intent;
  const dismiss=()=>{goal.dismiss();triggerRef.current?.focus();};
  return <div className="inline-flex min-w-0 max-w-full">
    <ComposerContextChip kind="scheduled" open={open} ref={triggerRef} aria-label="Session goal" title={(current?.goal || current?.title || "Session goal").split("\n")[0]} aria-expanded={open} aria-haspopup="dialog" onClick={toggle}>
      <Target size={13} aria-hidden="true" />
    </ComposerContextChip>
    {open && <div ref={popoverRef} role="dialog" aria-label="Session goal" tabIndex={-1} className="ct-menu absolute bottom-full right-0 z-40 mb-2 w-64 max-w-[calc(100vw-2rem)] whitespace-pre-wrap break-words p-3 text-xs text-[var(--ct-fg)]">
      {current?.title && <p>{current.title}</p>}
      {current?.goal ? <><p>{current.goal}</p><p role="status" aria-label="Goal status">{goalStatusLabel(current.goalStatus)}</p></> : <p role="status" aria-label="Goal status">No current goal</p>}
      {goal.error && !goal.confirming && <InlineErrorNotice error={goal.error} operation="reading the session goal" blocking />}
      <div role="group" aria-label="Goal actions" className="mt-3 flex flex-wrap gap-2">
        {goalActions(goal.snapshot).map(action=><button type="button" key={action} disabled={goal.busy} onClick={()=>goal.select(action)} className="rounded border border-[var(--ct-border)] px-2 py-1">{goalActionLabels[action]}</button>)}
      </div>
    </div>}
    {goal.confirming && <JobConfirmationDialog title={goalActionLabels[goal.confirming.action]} description={`${goalConfirmation(goal.confirming.action)}\n\n${goal.confirming.snapshot.supported ? goal.confirming.snapshot.intent?.goal ?? "" : ""}`} confirmLabel={goalActionLabels[goal.confirming.action]} busy={goal.busy} error={goal.error} onCancel={dismiss} onConfirm={()=>void goal.confirm()}/>}
  </div>;
}
