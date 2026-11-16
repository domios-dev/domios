import type { ReactNode } from "react";
import type { ProviderDescriptor } from "../../lib/types";
import { NO_HOST_PEER } from "../../lib/teamTemplates/hostPeers";
import { Button } from "../shared/Button";
import { DropdownSelect } from "../shared/DropdownMenu";
import type { TeamTemplateChoice } from "./useTeamTemplateChoice";
import { NEW_PEER_WORKSPACE, type PeerPlacement } from "./usePeerPlacement";

type TemplatePreview = import("../../lib/transport/teamTemplates").TemplatePreview;

const HOME = "";

function IssueLine({ text, children }: { text: string; children?: ReactNode }) {
  return (
    <div className="flex flex-wrap items-center gap-2" data-testid="template-where-issue">
      <span aria-hidden="true" className="h-1.5 w-1.5 shrink-0 rounded-full bg-[var(--ct-red)]" />
      <span className="text-[var(--ct-fg)]">{text}</span>
      {children}
    </div>
  );
}

/** One placement row: a no-wrap label in the 140 px column, its control aligned in the next. */
function PlacementRow({ label, select }: { label: string; select: ReactNode }) {
  return <>
    <span className="truncate whitespace-nowrap" title={label}>{label}</span>
    <div className="min-w-0">{select}</div>
  </>;
}

/** The peer workspace choices: its own workspaces, and a new one unless the package needs an existing project. */
function workspaceOptions(placement: PeerPlacement, needsProject: boolean) {
  return [
    ...(placement.workspaces ?? []).map((workspace) => ({ value: workspace.id, label: workspace.name })),
    ...(needsProject ? [] : [{ value: NEW_PEER_WORKSPACE, label: "New workspace" }]),
  ];
}

/**
 * Where the whole Team runs: this machine, or a paired machine and one of its own workspaces. One quiet link opens the
 * block; a peer's managers appear here as remote tabs once they start.
 */
export function TemplateWhere({ choice, preview, open, onOpen, showLink = true }: {
  choice: TeamTemplateChoice;
  preview: TemplatePreview;
  open: boolean;
  onOpen: () => void;
  showLink?: boolean;
}) {
  const { placement, remotePeer } = choice;
  const peers = placement.peers;
  const options = [{ value: HOME, label: "This machine" }, ...(peers ?? []).map((peer) => ({ value: peer.id, label: peer.name }))];
  const muted = "text-[11px] text-[var(--ct-muted-fg)]";
  return (
    <section aria-label="Where" className="flex flex-col gap-1.5 text-[12px]" data-testid="template-where">
      {!open && showLink && <Button type="button" variant="outline" className="self-start text-xs" onClick={onOpen}>Run on another machine</Button>}
      {open && (
        <div role="group" aria-label="Placement" className="flex flex-col gap-1.5">
          <div className="text-[11px] font-medium text-[var(--ct-muted-fg)]">Where it runs</div>
          {peers !== null && peers.length === 0 && !remotePeer
            ? <p role="status" className={muted} data-testid="template-where-reason">{placement.peersReason ?? NO_HOST_PEER}</p>
            : (
              <div className="grid grid-cols-[140px_minmax(0,176px)] items-center gap-x-3 gap-y-1.5" data-testid="template-where-grid">
                <PlacementRow label="Start on" select={
                  <DropdownSelect aria-label="Machine for this team" options={options}
                    value={remotePeer?.id ?? HOME} disabled={peers === null} onChange={(peer) => choice.setRemotePeer(peer || null)} />} />
                {remotePeer && <PlacementRow label={`Workspace on ${remotePeer.name}`} select={
                  <DropdownSelect aria-label={`Workspace on ${remotePeer.name}`} options={workspaceOptions(placement, preview.needs_project === true)}
                    value={placement.workspace} placeholder={placement.workspaces === null ? "Loading…" : "Choose a workspace"}
                    disabled={placement.workspaces === null} onChange={placement.setWorkspace} />} />}
                {remotePeer && placement.workspace === NEW_PEER_WORKSPACE && <PlacementRow label="Name" select={
                  <input aria-label={`New workspace name on ${remotePeer.name}`} value={placement.newName} onChange={(event) => placement.setNewName(event.target.value)}
                    className="w-full rounded border border-[var(--ct-border)] bg-transparent px-2 py-1 text-[12px]" placeholder="Workspace name" />} />}
              </div>
            )}
          {remotePeer && placement.workspacesReason && <p role="status" className={muted}>{placement.workspacesReason}</p>}
          {remotePeer && preview.needs_project === true && <p className={muted}>
            This team works in an existing project, so pick one of {remotePeer.name}'s workspaces.
          </p>}
          {remotePeer && <p className={muted}>
            Its Orchestrator and Planner run on {remotePeer.name} and appear here as remote tabs once they start.
          </p>}
        </div>
      )}
    </section>
  );
}

export function TemplateWhereIssues({ choice, preview, providers, onInstallProvider, onRunElsewhere }: {
  choice: TeamTemplateChoice;
  preview: TemplatePreview;
  providers: readonly ProviderDescriptor[];
  onInstallProvider: (providerId: string) => void;
  onRunElsewhere: () => void;
}) {
  const needs = choice.remotePeer ? [] : preview.where_needed?.needs ?? [];
  const harnessName = (id: string) => providers.find((provider) => provider.id === id)?.display_name ?? id;
  if (needs.length === 0) return null;
  return <div className="flex flex-col gap-1.5 text-[12px]">
      {needs.map((need) => need.kind === "missing_harness" ? (
        <IssueLine key={need.harness} text={`${harnessName(need.harness)} isn't installed on this machine`}>
          <Button type="button" variant="outline" className="text-xs" onClick={() => onInstallProvider(need.harness)}>Install</Button>
        </IssueLine>
      ) : <IssueLine key={need.tool} text={`${need.tool} isn't available on this machine`} />)}
    <Button type="button" variant="outline" className="self-start text-xs" onClick={onRunElsewhere}>Run on another machine</Button>
  </div>;
}
