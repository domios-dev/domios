import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useState, type CSSProperties } from "react";
import { Clock3 } from "lucide-react";
import { BackPill } from "../shared/BackPill";
import { CronDrawerSection } from "../MeshPanel/CronDrawerSection";
import type { MeshResourceActionAdapter } from "../MeshPanel/meshResourceActions";
import { useCronDetails } from "./useCronDetails";
export { selectCronService } from "./useCronDetails";
import { cronSelectionStore } from "./cronSelectionStore";
import { HostAutomationGraph } from "../Automation/HostAutomationGraph";
import { CronFlowView } from "./CronFlowView";
import { AutomationGraphView, type AutomationGraphViewProps } from "../Automation/AutomationGraphView";
import { AUTOMATION_TONE, AUTOMATION_TOOL } from "../Automation/styles";

interface Props {
  cronId: string;
  actionAdapter?: MeshResourceActionAdapter;
  automation?: AutomationGraphViewProps;
  /** Embedded inside Jobs: closing returns to the Jobs list instead of clearing the global cron selection. */
  backLabel?: string;
  onBack?: () => void;
}

export function CronWorkspaceView({ cronId, actionAdapter, automation, backLabel, onBack }: Props) {
  const [showAutomation, setShowAutomation] = useState(false);
  const { job, service, error } = useCronDetails(cronId);

  const back = onBack
    ? <BackPill label={backLabel} onBack={onBack} />
    : <BackPill ariaLabel="Close cron details" onBack={() => cronSelectionStore.close()} />;
  if (error && !job) {
    return <div className="h-full grid place-items-center gap-3 text-sm text-[var(--ct-red)]"><InlineErrorNotice error={error} operation="loading scheduled jobs" blocking />{back}</div>;
  }
  if (!job || !service) return <div className="p-4">{back}</div>;

  const target = job.report_target_tab_id
    ? `Reports to Tab ${job.report_target_tab_id}`
    : "Runs in a private cron worker";
  // A Team's cron opens its Team's automation graph; a cron without a Team opens its own small flow.
  const teamId = typeof job.team_id === "string" && job.team_id ? job.team_id : null;
  const graphShown = showAutomation;

  return (
    <section className="h-full overflow-y-auto bg-[var(--ct-bg)]" aria-label={job.role === "curator" ? "Curator rounds" : `Cron ${job.name}`}>
      <header className="ct-jobs-header sticky top-0 flex flex-wrap items-center gap-x-3 gap-y-2 px-4 py-3 sm:px-6">
        {back}
        <span className="ct-card__icon" style={{ "--ct-card-tone": AUTOMATION_TONE.cron } as CSSProperties}><Clock3 size={14} aria-hidden="true" /></span>
        <div className="min-w-0 flex-[1_1_12rem]">
          <h2 className="break-words text-[length:var(--ct-font-size-md)] font-semibold text-[var(--ct-fg)]">{job.role === "curator" ? "Curator rounds" : job.name}</h2>
          <p className="text-xs text-[var(--ct-muted-fg)]">Cron · {target}</p>
        </div>
        <button type="button" aria-pressed={graphShown} onClick={() => setShowAutomation(!graphShown)} className={AUTOMATION_TOOL}>{graphShown ? "Cron details" : "Automation graph"}</button>
      </header>
      {graphShown ? (automation ? <AutomationGraphView {...automation} />
        : teamId ? <HostAutomationGraph workspace_id={job.workspace_id} team_id={teamId} />
        : <CronFlowView job={job} service={service} target={target} actionAdapter={actionAdapter} />) : <div className="mx-auto w-full max-w-3xl px-4 py-5 sm:px-6 sm:py-6">
        <CronDrawerSection service={service} machineName="This machine" onUpdate={() => void refresh()} actionAdapter={actionAdapter} />
      </div>}
    </section>
  );
}
