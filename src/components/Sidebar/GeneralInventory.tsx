import { useId, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import type { Tab, Team, Workspace } from "../../lib/types";
import { shellPurposeLabel, shellStateLabel } from "../../lib/shellCommandState";
import { generalInventorySections, projectUtilityShells } from "../General/generalOverviewModel";
import { ServiceDrawer, type DrawerState } from "../MeshPanel/ServiceDrawer";
import { useMeshData, type MeshDataResult } from "../MeshPanel/useMeshData";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { SidebarDisclosure } from "./SidebarDisclosure";
import { useGeneralJobs } from "./useGeneralJobs";
import { jobStateLabel, mergedJobId, type MergedJob } from "../Jobs/jobsModel";
import { openTabJob } from "../Jobs/useTabJobs";

const STORAGE_KEY = "codeterm.sidebar.general-disclosures";
type GroupName = "Jobs" | "Services" | "Utility shells";

function readExpanded(name: GroupName): boolean {
  try { return JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}")[name] === true; }
  catch { return false; }
}

function InventoryGroup({ name, count, children, touchMode }: { name: GroupName; count: number; children: ReactNode; touchMode: boolean }) {
  const [expanded, setExpanded] = useState(() => readExpanded(name));
  const id = useId();
  const open = count > 0 && expanded;
  return <section aria-label={name} className="min-w-0 px-2">
    <SidebarDisclosure expanded={open} aria-label={`${name}, ${count}`} aria-controls={id} disabled={count === 0} touchMode={touchMode}
      className="sidebar-structural-toggle w-full min-w-0"
      onClick={() => {
        const next = !expanded;
        setExpanded(next);
        try {
          const states = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}");
          localStorage.setItem(STORAGE_KEY, JSON.stringify({ ...states, [name]: next }));
        } catch { /* Storage can be unavailable in a private viewer. */ }
      }}>
      <span className="min-w-0 flex-1 truncate text-left">{name}</span><span className="tabular-nums text-[var(--ct-muted)]">{count}</span>
    </SidebarDisclosure>
    <ul id={id} aria-label={name} hidden={!open} className="min-w-0 pl-3">{children}</ul>
  </section>;
}

function InventoryRow({ name, label = name, detail, onClick, touchMode }: { name: string; label?: string; detail: string; onClick: () => void; touchMode: boolean }) {
  return <li className="min-w-0"><button type="button" aria-label={`Open ${name}`} onClick={onClick}
    className={`w-full min-w-0 rounded px-2 py-1 text-left text-xs hover:bg-[var(--ct-hover)] focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)] ${touchMode ? "min-h-11" : "min-h-8"}`}>
    <span className="block truncate" title={label}>{label}</span><span className="block truncate text-[var(--ct-muted-fg)]" title={detail}>{detail}</span>
  </button></li>;
}

export interface GeneralInventoryProps {
  workspaces: readonly Workspace[];
  teams: readonly Team[];
  tabs: Record<string, Tab>;
  onNavigate: (workspaceId: string, tabId: string) => void;
  touchMode?: boolean;
}

export function GeneralInventorySections({ workspaces, teams, tabs, onNavigate, touchMode = false, inventory, jobs, jobsFailed = false, onOpenJob = openTabJob }: GeneralInventoryProps & {
  inventory: MeshDataResult;
  jobs: readonly MergedJob[];
  jobsFailed?: boolean;
  onOpenJob?: (job: MergedJob) => void;
}) {
  const [drawer, setDrawer] = useState<DrawerState>(null);
  const sections = generalInventorySections({ resources: inventory.services, loading: inventory.loading, error: inventory.error }, inventory.thisMachine.name);
  const shells = projectUtilityShells(workspaces, tabs, teams);
  const remoteSchedules = sections.remoteSchedules;
  const resourceRows = (rows: typeof sections.services.rows) => rows.map((row) => <InventoryRow key={row.resourceKey}
    name={`${row.resource.name}, machine ${row.machineName}`} label={row.resource.name} detail={`${row.machineName} · ${row.stateLabel} · ${row.freshness}${row.schedule ? ` · ${row.schedule}` : ""}`} touchMode={touchMode}
    onClick={() => setDrawer({ kind: "service", id: row.resource.id, sourcePeerId: row.resource.source_peer_id ?? row.resource.source_peer })} />);
  return <div className="min-w-0">
    <InventoryGroup name="Jobs" count={jobs.length + remoteSchedules.length} touchMode={touchMode}>
      {jobs.map((job) => <InventoryRow key={`${job.kind}:${mergedJobId(job)}`} name={`job ${job.title}`} label={job.title} detail={jobStateLabel(job.state)} touchMode={touchMode} onClick={() => onOpenJob(job)} />)}
      {resourceRows(remoteSchedules)}
    </InventoryGroup>
    <InventoryGroup name="Services" count={sections.services.rows.length} touchMode={touchMode}>{resourceRows(sections.services.rows)}</InventoryGroup>
    <InventoryGroup name="Utility shells" count={shells.length} touchMode={touchMode}>
      {shells.map((shell) => <InventoryRow key={shell.tab.id} name={`utility shell ${shell.tab.title}`} label={shell.tab.title} touchMode={touchMode}
        detail={`${shell.workspace.name} · ${shell.team?.name ?? "No Team"} · ${shell.owner?.name ?? "User"} · ${shellPurposeLabel(shell.tab.purpose)} · ${shellStateLabel(shell.tab.shell_command_state)}`}
        onClick={() => onNavigate(shell.target.workspaceId, shell.target.tabId)} />)}
    </InventoryGroup>
    {jobsFailed && <InlineErrorNotice error={{ code: "jobs_unavailable", raw: "" }} />}
    {inventory.error && <InlineErrorNotice error={inventory.error} />}
    {drawer && createPortal(<div className="[&_[role=dialog]]:max-w-[calc(100vw-2rem)]"><ServiceDrawer state={drawer} services={inventory.services} peers={inventory.peers} thisMachineName={inventory.thisMachine.name} thisMachineIp={inventory.thisMachine.ip} onClose={() => setDrawer(null)} onActionDone={inventory.refresh} /></div>, document.body)}
  </div>;
}

export function GeneralInventory(props: GeneralInventoryProps) {
  const inventory = useMeshData(0);
  const { jobs, failed } = useGeneralJobs(props.workspaces);
  return <GeneralInventorySections {...props} inventory={inventory} jobs={jobs} jobsFailed={failed} />;
}
