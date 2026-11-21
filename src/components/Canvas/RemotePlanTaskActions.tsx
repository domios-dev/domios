import { useId, useRef, useState } from "react";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import type { TeamMissionControlState } from "./missionControl/missionControlState";
import { remotePlanGrantReason, runRemotePlanAction, type RemoteTeamPlanTarget } from "./remoteTeamPlanModel";
import type { PlanTaskDetail } from "./types";

export function RemotePlanTaskActions({ target, detail, grant, workspaceId }: {
  target: RemoteTeamPlanTarget;
  detail: PlanTaskDetail;
  grant: TeamMissionControlState["remote_plan_actions"];
  workspaceId: string;
}) {
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [receipt, setReceipt] = useState<string | null>(null);
  const pending = useRef(false);
  const reasonId = useId();
  const noteId = useId();
  const grantReason = remotePlanGrantReason(grant);
  const reviewReason = grantReason ?? (detail.task.status === "in_review" ? null : "The task is not in review.");
  const dispatchReason = grantReason ?? (["pending", "blocked"].includes(detail.task.status) ? null : "The task is not pending or blocked.");
  const run = async (action: "plan.review.accept" | "plan.review.bounce" | "plan.task.dispatch") => {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    setError(null);
    setReceipt(null);
    try {
      const fields = action === "plan.task.dispatch"
        ? { execution_intent: detail.task.execution_intent === "non_repository" ? "read_only" as const : "mutating" as const, workspace_id: workspaceId }
        : { note: note.trim() };
      await runRemotePlanAction(target, grant, detail.plan_id, detail.task_id, action, fields);
      setReceipt(action === "plan.task.dispatch" ? "Dispatch request accepted." : "Review recorded.");
      setNote("");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      pending.current = false;
      setBusy(false);
    }
  };
  return <div className="space-y-2 px-4 py-3 text-xs">
    <h3 className="font-semibold">Task actions</h3>
    {grantReason && <p id={reasonId}>Read-only. {grantReason}</p>}
    {!reviewReason && <label htmlFor={noteId} className="block">Review note
      <textarea id={noteId} className="mt-1 block w-full rounded border p-2" value={note} onChange={event => setNote(event.target.value)} />
    </label>}
    <div className="flex flex-wrap gap-2">
      <button type="button" className="rounded border px-3 py-1" disabled={Boolean(reviewReason) || !note.trim() || busy}
        title={reviewReason ?? (!note.trim() ? "Enter a review note." : undefined)}
        aria-describedby={grantReason ? reasonId : undefined} onClick={() => void run("plan.review.accept")}>Accept review</button>
      <button type="button" className="rounded border px-3 py-1" disabled={Boolean(reviewReason) || !note.trim() || busy}
        title={reviewReason ?? (!note.trim() ? "Enter a review note." : undefined)}
        aria-describedby={grantReason ? reasonId : undefined} onClick={() => void run("plan.review.bounce")}>Bounce review</button>
      <button type="button" className="rounded border px-3 py-1" disabled={Boolean(dispatchReason) || busy}
        title={dispatchReason ?? undefined} aria-describedby={grantReason ? reasonId : undefined}
        onClick={() => void run("plan.task.dispatch")}>Dispatch task</button>
    </div>
    {error && <InlineErrorNotice role="alert" error={error} operation="updating this task on its owning machine" />}
    {receipt && <p role="status">{receipt}</p>}
  </div>;
}
