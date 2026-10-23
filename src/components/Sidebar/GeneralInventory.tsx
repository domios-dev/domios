import { useId, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import type { Tab, Team, Workspace } from "../../lib/types";
import { Circle, Clock3 } from "lucide-react";
import { SidebarCronRow, SidebarResourceRow } from "./SidebarResourceRow";
import { DropdownMenu } from "../shared/DropdownMenu";
import { RowMenuButton } from "../shared/RowMenuButton";
import { SidebarShellRow, type SidebarOwnerTabRowProps } from "./SidebarShellRow";
import { surfaceAttachmentsByOwner } from "./surfaceAttachmentModel";
import { generalInventorySections, generalWorkspaceJobs, projectUtilityShells } from "../General/generalOverviewModel";
import { ServiceDrawer, type DrawerState } from "../MeshPanel/ServiceDrawer";
import { useMeshData, type MeshDataResult } from "../MeshPanel/useMeshData";
import { SidebarSectionHeader } from "./SidebarSectionHeader";
import { tabActivity } from "../../lib/tabActivity";
import { inventoryGroupVisibility } from "./generalInventoryModel";
import { useGeneralJobs } from "./useGeneralJobs";
import { jobStateLabel, mergedJobId, type MergedJob } from "../Jobs/jobsModel";
import { openTabJob } from "../Jobs/useTabJobs";

const STORAGE_KEY = "codeterm.sidebar.general-disclosures";
type GroupName = "Jobs" | "Services" | "Utility shells";

function readExpanded(name: GroupName): boolean {
  try { return JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}")[name] === true; }
  catch { return false; }
}

function InventoryGroup({ name, count, working = 0, children, touchMode, onShowAll }: { onShowAll?: () => void; name: GroupName; count: number; working?: number; children: ReactNode; touchMode: boolean }) {
  const [expanded, setExpanded] = useState(() => readExpanded(name));
  const id = useId();
  const { visible, open, showAll } = inventoryGroupVisibility(count, expanded, !!onShowAll);
  if (!visible) return null;
  return <section aria-label={name} className="sidebar-team-section min-w-0">
    <SidebarSectionHeader kind={name === "Utility shells" ? "utilities" : name === "Jobs" ? "jobs" : "services"} count={count} working={working} expanded={open} aria-controls={id} touchMode={touchMode}
      onClick={() => {
        const next = !expanded;
        setExpanded(next);
        try {
          const states = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}");
          localStorage.setItem(STORAGE_KEY, JSON.stringify({ ...states, [name]: next }));
        } catch { /* Storage can be unavailable in a private viewer. */ }
      }} />
    <ul id={id} aria-label={name} hidden={!open} className="sidebar-team-member-list min-w-0">{children}{showAll && onShowAll && <li><SidebarResourceRow name="Show all" state="" icon={<Clock3 size={11} aria-hidden="true" />} accessibleName="Show all jobs" onOpen={onShowAll} /></li>}</ul>
  </section>;
}

export interface GeneralInventoryProps {
  generalWorkspaceId: string;
  onShowAllJobs?: () => void;
  workspaces: readonly Workspace[];
  teams: readonly Team[];
  tabs: Record<string, Tab>;
  onNavigate: (workspaceId: string, tabId: string) => void;
  touchMode?: boolean;
  onCloseTab: (workspaceId: string, tabId: string) => void;
  onRenameTab: (workspaceId: string, tabId: string, title: string) => void;
  rowState?: Pick<SidebarOwnerTabRowProps, "tabStatuses" | "tabAttentionByTab" | "planIndex" | "closingTabIds" | "appearingTabIds" | "orchestrators" | "uiFontScale">;
}

export function GeneralInventorySections({ generalWorkspaceId, onShowAllJobs, workspaces, teams, tabs, onNavigate, touchMode = false, onCloseTab, onRenameTab, rowState, inventory, jobs, onOpenJob = openTabJob, onJobsChanged = () => {} }: GeneralInventoryProps & {
  inventory: MeshDataResult;
  jobs: readonly MergedJob[];
  jobsFailed?: boolean;
  onOpenJob?: (job: MergedJob) => void;
  onJobsChanged?: () => void;
}) {
  const [drawer, setDrawer] = useState<DrawerState>(null);
  const sections = generalInventorySections({ resources: inventory.services, loading: inventory.loading, error: inventory.error }, inventory.thisMachine.name);
  const shells = projectUtilityShells(workspaces, tabs, teams, generalWorkspaceId);
  const localJobs = generalWorkspaceJobs(jobs, generalWorkspaceId);
  const attachmentsByOwner = surfaceAttachmentsByOwner(Object.values(tabs));
  const resourceRows = (rows: typeof sections.services.rows) => rows.map((row) => <li key={row.resourceKey}>
    <SidebarResourceRow name={row.resource.name} state={row.stateLabel}
      accessibleName={`Open ${row.resource.name}, machine ${row.machineName}`}
      title={`${row.machineName} · ${row.stateLabel} · ${row.freshness}${row.schedule ? ` · ${row.schedule}` : ""}`}
      icon={row.schedule ? <Clock3 size={11} aria-hidden="true" /> : <Circle size={7} fill="currentColor" aria-hidden="true" />}
      onOpen={() => setDrawer({ kind: "service", id: row.resource.id, sourcePeerId: row.resource.source_peer_id ?? row.resource.source_peer })} />
  </li>);
  return <div className="min-w-0">
    <InventoryGroup name="Jobs" count={localJobs.length} working={localJobs.filter(job => job.active).length} touchMode={touchMode} onShowAll={onShowAllJobs}>
      {localJobs.map((job) => <li key={`${job.kind}:${mergedJobId(job)}`}>
        {job.kind === "cron" ? <SidebarCronRow job={job.cron} tabs={tabs} onDeleted={onJobsChanged} />
          : <SidebarResourceRow name={job.title} state={jobStateLabel(job.state)} icon={<Clock3 size={11} aria-hidden="true" />}
            accessibleName={`Open job ${job.title}`} onOpen={() => onOpenJob(job)}
            actions={<DropdownMenu className="sidebar-team-cron-menu" menuLabel="Job actions"
              items={[{ label: "System job cannot be changed", disabled: true, title: "System job cannot be changed" }]}
              trigger={<RowMenuButton label={`Job actions for ${job.title}`} title="Job actions" />} />} />}
      </li>)}
    </InventoryGroup>
    <InventoryGroup name="Services" count={sections.services.rows.length} working={sections.services.rows.filter(row => row.state === "running").length} touchMode={touchMode}>{resourceRows(sections.services.rows)}</InventoryGroup>
    <InventoryGroup name="Utility shells" count={shells.length} working={shells.filter(shell => tabActivity(shell.tab, rowState?.tabStatuses[shell.tab.id]).active).length} touchMode={touchMode}>
      {shells.map((shell, index) => <li key={shell.tab.id}>
        <SidebarShellRow tab={shell.tab} index={index} tabs={tabs}
          attachments={attachmentsByOwner.get(shell.tab.id) ?? []}
          planIndex={rowState?.planIndex ?? []}
          closingTabIds={rowState?.closingTabIds ?? new Set()} appearingTabIds={rowState?.appearingTabIds ?? new Set()}
          compact={false} sortable={false} group="team" groupKey={shell.tab.team_id ?? shell.workspace.id} pinnedIds={[]}
          workspaceId={shell.workspace.id} activeTabId={null} isActiveVisual={false} workspaceCanvasActive={false}
          tabStatuses={rowState?.tabStatuses ?? {}} tabAttentionByTab={rowState?.tabAttentionByTab}
          orchestrators={rowState?.orchestrators ?? []} touchMode={touchMode} uiFontScale={rowState?.uiFontScale}
          onActivateTab={(tabId) => {
            const workspace = workspaces.find((entry) => entry.tabs.includes(tabId));
            if (workspace) onNavigate(workspace.id, tabId);
          }}
          onCloseTab={(tabId) => onCloseTab(shell.workspace.id, tabId)}
          onRenameTab={(tabId, title) => onRenameTab(shell.workspace.id, tabId, title)} />
      </li>)}
    </InventoryGroup>
    {drawer && createPortal(<div className="[&_[role=dialog]]:max-w-[calc(100vw-2rem)]"><ServiceDrawer state={drawer} services={inventory.services} peers={inventory.peers} thisMachineName={inventory.thisMachine.name} thisMachineIp={inventory.thisMachine.ip} onClose={() => setDrawer(null)} onActionDone={inventory.refresh} /></div>, document.body)}
  </div>;
}

export function GeneralInventory(props: GeneralInventoryProps) {
  const inventory = useMeshData(0);
  const { jobs, failed, refresh } = useGeneralJobs(props.workspaces, props.generalWorkspaceId);
  return <GeneralInventorySections {...props} inventory={inventory} jobs={jobs} jobsFailed={failed} onJobsChanged={refresh} />;
}
