import { useEffect, useState, type CSSProperties } from "react";
import { Clock3 } from "lucide-react";
import { BackPill } from "../shared/BackPill";
import { invoke, listen } from "../../lib/bridge";
import type { CronJob } from "../../lib/types";
import { CronDrawerSection } from "../MeshPanel/CronDrawerSection";
import type { MeshResourceActionAdapter } from "../MeshPanel/meshResourceActions";
import { CRON_ID_PREFIX, type Service } from "../MeshPanel/types";
import type { MeshInventoryView } from "../MeshPanel/servicesView";
import { cronSelectionStore } from "./cronSelectionStore";
import { HostAutomationGraph } from "../Automation/HostAutomationGraph";
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

// Same inventory projection MeshPanel renders from — never a hand-built capability map.
export function selectCronService(inventory: MeshInventoryView, cronId: string): Service | null {
  const resourceId = `${CRON_ID_PREFIX}${cronId}`;
  return inventory.resources.find((resource) => resource.id === resourceId) ?? null;
}

export function CronWorkspaceView({ cronId, actionAdapter, automation, backLabel, onBack }: Props) {
  const [showAutomation, setShowAutomation] = useState(false);
  const [job, setJob] = useState<CronJob | null>(null);
  const [service, setService] = useState<Service | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = async () => {
    try {
      const [nextJob, inventory] = await Promise.all([
        invoke<CronJob>("cron_get", { id: cronId }),
        invoke<MeshInventoryView>("mesh_inventory_list"),
      ]);
      setJob(nextJob);
      setService(selectCronService(inventory, cronId));
      setError(null);
    } catch (cause) {
      setError(String(cause));
    }
  };

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    void refresh();
    void listen("cron-run-finished", () => {
      if (active) void refresh();
    }).then((stop) => {
      if (active) unlisten = stop;
      else stop();
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [cronId]);

  const back = onBack
    ? <BackPill label={backLabel} onBack={onBack} />
    : <BackPill ariaLabel="Close cron details" onBack={() => cronSelectionStore.close()} />;
  if (error) {
    return <div className="h-full grid place-items-center gap-3 text-sm text-[var(--ct-red)]"><p role="alert">{error}</p>{back}</div>;
  }
  if (!job || !service) return <div className="p-4">{back}</div>;

  const target = job.report_target_tab_id
    ? `Reports to Tab ${job.report_target_tab_id}`
    : "Runs in a private cron worker";

  return (
    <section className="h-full overflow-y-auto bg-[var(--ct-bg)]" aria-label={`Cron ${job.name}`}>
      <header className="ct-jobs-header sticky top-0 flex flex-wrap items-center gap-x-3 gap-y-2 px-4 py-3 sm:px-6">
        {back}
        <span className="ct-card__icon" style={{ "--ct-card-tone": AUTOMATION_TONE.cron } as CSSProperties}><Clock3 size={14} aria-hidden="true" /></span>
        <div className="min-w-0 flex-[1_1_12rem]">
          <h2 className="break-words text-[length:var(--ct-font-size-md)] font-semibold text-[var(--ct-fg)]">{job.name}</h2>
          <p className="text-xs text-[var(--ct-muted-fg)]">Cron · {target}</p>
        </div>
        <button type="button" aria-pressed={showAutomation} onClick={() => setShowAutomation(!showAutomation)} className={AUTOMATION_TOOL}>{showAutomation ? "Cron details" : "Automation graph"}</button>
      </header>
      {showAutomation ? (automation ? <AutomationGraphView {...automation} /> : job.team_id === undefined ? <p role="alert" className="px-4 py-3 sm:px-6">Automation Team scope is unavailable.</p> : <HostAutomationGraph workspace_id={job.workspace_id} team_id={job.team_id} />) : <div className="mx-auto w-full max-w-3xl px-4 py-5 sm:px-6 sm:py-6">
        <CronDrawerSection service={service} machineName="This machine" onUpdate={() => void refresh()} actionAdapter={actionAdapter} />
      </div>}
    </section>
  );
}
