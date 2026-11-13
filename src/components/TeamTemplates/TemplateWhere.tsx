import type { ReactNode } from "react";
import type { ProviderDescriptor } from "../../lib/types";
import { roleDisplayLabel } from "../../lib/roleKind";
import { Button } from "../shared/Button";
import { DropdownSelect } from "../shared/DropdownMenu";
import type { TeamTemplateChoice } from "./useTeamTemplateChoice";

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

/** One placement row: a no-wrap label in the 140 px column, its select aligned in the next. */
function PlacementRow({ label, select, muted = false }: { label: string; select: ReactNode; muted?: boolean }) {
  return <>
    <span className={`truncate whitespace-nowrap ${muted ? "text-[var(--ct-muted-fg)]" : ""}`} title={label}>{label}</span>
    <div className="min-w-0">{select}</div>
  </>;
}

/**
 * Where the Team runs: this machine unless the owner moves some workers, or the whole Team, to a peer. One quiet link
 * opens the block; the whole-team choice lives inside it.
 */
export function TemplateWhere({ choice, preview, open, onOpen, showLink = true }: {
  choice: TeamTemplateChoice;
  preview: TemplatePreview;
  open: boolean;
  onOpen: () => void;
  showLink?: boolean;
}) {
  const workers = preview.form.roles.filter((role) => role.kind === "worker");
  const peers = choice.peers;
  const options = [{ value: HOME, label: "This machine" }, ...(peers ?? []).map((peer) => ({ value: peer.name, label: peer.name }))];
  return (
    <section aria-label="Where" className="flex flex-col gap-1.5 text-[12px]" data-testid="template-where">
      {!open && showLink && <Button type="button" variant="outline" className="self-start text-xs" onClick={onOpen}>Run on another machine</Button>}
      {open && (
        <div role="group" aria-label="Placement" className="flex flex-col gap-1.5">
          <div className="text-[11px] font-medium text-[var(--ct-muted-fg)]">Where it runs</div>
          {peers !== null && peers.length === 0 && !choice.remotePeer
            ? <p className="text-[11px] text-[var(--ct-muted-fg)]">No paired machine with Full access is online.</p>
            : (
              <div className="grid grid-cols-[140px_minmax(0,176px)] items-center gap-x-3 gap-y-1.5" data-testid="template-where-grid">
                {!choice.remotePeer && workers.map((role) => {
                  const label = roleDisplayLabel(role.role, role.label);
                  return <PlacementRow key={role.role} label={label} select={
                    <DropdownSelect aria-label={`Machine for ${label}`} options={options}
                      value={choice.placements[role.role] ?? HOME} disabled={peers === null} onChange={(peer) => choice.setPlacement(role.role, peer)} />} />;
                })}
                <PlacementRow label={workers.length > 0 ? "Whole team" : "This team"} muted={workers.length > 0 && !choice.remotePeer} select={
                  <DropdownSelect aria-label="Machine for this team" options={options}
                    value={choice.remotePeer ?? HOME} disabled={peers === null} onChange={(peer) => choice.setRemotePeer(peer || null)} />} />
              </div>
            )}
          {choice.remotePeer && <p className="text-[11px] text-[var(--ct-muted-fg)]">
            Its managers run on {choice.remotePeer}. Nothing starts until {choice.remotePeer}'s owner taps Start here.
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
