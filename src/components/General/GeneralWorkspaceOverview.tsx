import { useState } from "react";
import { shellPurposeLabel, shellStateLabel } from "../../lib/shellCommandState";
import type { Tab, Team, Workspace } from "../../lib/types";
import { JobsView, type JobsViewProps } from "../Jobs/JobsView";
import { ServiceDrawer, type DrawerState } from "../MeshPanel/ServiceDrawer";
import { useMeshData, type MeshDataResult } from "../MeshPanel/useMeshData";
import type { InventoryModel } from "../MeshPanel/meshInventoryModel";
import type { Service } from "../MeshPanel/types";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { generalInventorySections, projectUtilityShells } from "./generalOverviewModel";

export interface GeneralWorkspaceOverviewProps {
  workspaces: readonly Workspace[];
  teams: readonly Team[];
  tabs: Record<string, Tab>;
  onNavigate: (workspaceId: string, tabId: string) => void;
  jobsProps?: Pick<JobsViewProps, "adapter" | "cronActionAdapter" | "jobActionAdapter">;
}

function InventorySection({ name, model, onOpen }: { name: string; model: InventoryModel; onOpen: (resource: Service) => void }) {
  return <section aria-label={name} className="min-w-0 border-t border-[var(--ct-border)] p-4">
    <h2 className="mb-3 font-semibold">{name}</h2>
    {model.rows.length === 0 && <p role="status" className="text-[var(--ct-muted-fg)]">{model.state === "loading" ? "Reading inventory…" : `No ${name.toLowerCase()}`}</p>}
    <ul aria-label={name} className="flex min-w-0 flex-col gap-2">
      {model.rows.map((row) => <li key={row.resourceKey}>
        <button type="button" className="w-full min-w-0 rounded border border-[var(--ct-border)] p-3 text-left hover:bg-[var(--ct-hover)]" aria-label={`Open ${row.resource.name}, machine ${row.machineName}`} onClick={() => onOpen(row.resource)}>
          <span className="block break-words font-medium">{row.resource.name}</span>
          <span className="block break-words text-xs text-[var(--ct-muted-fg)]">{row.machineName} · {row.stateLabel} · {row.freshness}{row.schedule ? ` · ${row.schedule}` : ""}</span>
          <span className="block break-words text-xs text-[var(--ct-muted-fg)]">{row.operationSummary}</span>
        </button>
      </li>)}
    </ul>
  </section>;
}

export function GeneralOverviewRail({ workspaces, teams, tabs, onNavigate, jobsProps, inventory }: GeneralWorkspaceOverviewProps & { inventory: MeshDataResult }) {
  const [drawer, setDrawer] = useState<DrawerState>(null);
  const sections = generalInventorySections({ resources: inventory.services, loading: inventory.loading, error: inventory.error }, inventory.thisMachine.name);
  const shells = projectUtilityShells(workspaces, tabs, teams);
  const openResource = (resource: Service) => setDrawer({ kind: "service", id: resource.id, sourcePeerId: resource.source_peer_id ?? resource.source_peer });
  return <section id="general-workspace-overview" aria-label="Workspace overview" className="h-full min-w-0 overflow-y-auto bg-[var(--ct-bg)] text-[var(--ct-fg)] [&_[role=dialog]]:max-w-[calc(100vw-2rem)]">
    <div className="min-h-80 min-w-0"><JobsView {...jobsProps} workspaces={workspaces} teams={teams} tabs={tabs} /></div>
    <div className="flex flex-wrap items-center gap-2 px-4 py-2">
      <button type="button" onClick={inventory.refresh} disabled={inventory.loading} className="rounded border border-[var(--ct-border)] px-3 py-1">Refresh inventory</button>
      {inventory.loading && <p role="status">Reading inventory…</p>}
      {inventory.error && <InlineErrorNotice error={inventory.error} />}
      {inventory.peers.filter((peer) => peer.services_error || peer.services_stale).map((peer) => <div key={peer.instance_id} className="min-w-0 text-sm"><p role="status" className="break-words">{peer.name}: {peer.online ? "stale inventory" : "offline"}</p>{peer.services_error && <InlineErrorNotice error={peer.services_error} />}</div>)}
    </div>
    <InventorySection name="Services" model={sections.services} onOpen={openResource} />
    <InventorySection name="Schedules" model={sections.schedules} onOpen={openResource} />
    <section aria-label="Utility shells" className="min-w-0 border-t border-[var(--ct-border)] p-4">
      <h2 className="mb-3 font-semibold">Utility shells</h2>
      {shells.length === 0 && <p role="status">No utility shells</p>}
      <ul aria-label="Utility shells" className="flex min-w-0 flex-col gap-2">
        {shells.map((shell) => <li key={shell.tab.id}>
          <button type="button" aria-label={`Open utility shell ${shell.tab.title}`} onClick={() => onNavigate(shell.target.workspaceId, shell.target.tabId)} className="w-full rounded border border-[var(--ct-border)] p-3 text-left hover:bg-[var(--ct-hover)]">
            <span className="block break-words font-medium">{shell.tab.title} · {shellPurposeLabel(shell.tab.purpose)}</span>
            <span className="block break-words text-xs text-[var(--ct-muted-fg)]">Workspace: {shell.workspace.name} · Team: {shell.team?.name ?? "None"} · Owner: {shell.owner?.name ?? "User"}</span>
            <span className="block break-words text-xs text-[var(--ct-muted-fg)]">{shellStateLabel(shell.tab.shell_command_state)}</span>
          </button>
        </li>)}
      </ul>
    </section>
    <ServiceDrawer state={drawer} services={inventory.services} peers={inventory.peers} thisMachineName={inventory.thisMachine.name} thisMachineIp={inventory.thisMachine.ip} onClose={() => setDrawer(null)} onActionDone={inventory.refresh} />
  </section>;
}

export function GeneralWorkspaceOverview(props: GeneralWorkspaceOverviewProps) {
  const inventory = useMeshData(0);
  return <GeneralOverviewRail {...props} inventory={inventory} />;
}
