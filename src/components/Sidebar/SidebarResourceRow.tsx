import type { ReactNode } from "react";
import { Clock3, X } from "lucide-react";
import type { CronJob, Tab } from "../../lib/types";
import { invoke } from "../../lib/bridge";
import { cronSelectionStore } from "../Cron/cronSelectionStore";
import { cronState, jobStateLabel } from "../Jobs/jobsModel";
import { DropdownMenu } from "../shared/DropdownMenu";
import { RowMenuButton } from "../shared/RowMenuButton";
import { cronRowMenu } from "./cronRowMenu";
import { cronReportTitle } from "./teamRowLabels";

export function SidebarResourceRow({ name, state, icon, title, accessibleName, onOpen, actions }: {
  name: string; state: string; icon: ReactNode; title?: string; accessibleName?: string;
  onOpen: () => void; actions?: ReactNode;
}) {
  return <div className="sidebar-team-cron-row">
    <button type="button" className="sidebar-team-cron-main" onClick={onOpen} title={title} aria-label={accessibleName}>
      {icon}
      <span className="sidebar-team-cron-name">{name}</span>
      <span className="sidebar-team-cron-state">{state}</span>
    </button>
    {actions}
  </div>;
}

export function SidebarCronRow({ job, tabs, onBeforeOpen, onDeleted }: {
  job: CronJob; tabs: Record<string, Tab>; onBeforeOpen?: () => void; onDeleted: () => void;
}) {
  return <SidebarResourceRow name={job.name} state={jobStateLabel(cronState(job))} accessibleName={`Open job ${job.name}`}
    icon={<Clock3 size={11} aria-hidden="true" />}
    title={cronReportTitle(job.report_target_tab_id, job.report_target_tab_id ? tabs[job.report_target_tab_id]?.title : null)}
    onOpen={() => {
      onBeforeOpen?.();
      cronSelectionStore.open({ cronId: job.id, workspaceId: job.workspace_id });
    }}
    actions={<DropdownMenu className="sidebar-team-cron-menu" menuLabel="Cron actions"
      items={cronRowMenu(job).map((item) => ({
        label: item.label, disabled: item.disabled, danger: item.action === "delete",
        icon: item.action === "delete" ? <X size={11} strokeWidth={2} /> : undefined,
        onClick: item.disabled ? undefined : () => {
          const request = item.action === "toggle"
            ? invoke("cron_toggle", { id: job.id, enabled: !job.enabled })
            : invoke("cron_delete", { id: job.id });
          void request.then(onDeleted);
        },
      }))}
      trigger={<RowMenuButton label={`Cron actions for ${job.name}`} title="Cron actions" />} />} />;
}
