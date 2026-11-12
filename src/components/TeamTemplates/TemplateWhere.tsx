import { useState } from "react";
import { DropdownSelect } from "../shared/DropdownMenu";
import type { TeamTemplateChoice } from "./useTeamTemplateChoice";

type TemplatePreview = import("../../lib/transport/teamTemplates").TemplatePreview;

const LINK = "self-start text-[12px] text-[var(--ct-accent)] hover:underline";
const HOME = "";

/** Where the Team runs: this machine unless the owner moves some workers, or the whole Team, to a peer. */
export function TemplateWhere({ choice, preview, onInstallProvider }: {
  choice: TeamTemplateChoice;
  preview: TemplatePreview;
  onInstallProvider: (providerId: string) => void;
}) {
  const [open, setOpen] = useState<"roles" | "team" | null>(choice.remotePeer ? "team" : null);
  const workers = preview.form.roles.filter((role) => role.group === "worker");
  const needs = choice.remotePeer ? [] : preview.where_needed?.needs ?? [];
  const peers = choice.peers;
  const options = [{ value: HOME, label: "This machine" }, ...(peers ?? []).map((peer) => ({ value: peer.name, label: peer.name }))];
  const reveal = (which: "roles" | "team") => {
    choice.wantPeers();
    setOpen(which);
  };
  const noPeers = peers !== null && peers.length === 0
    ? <p className="text-[11px] text-[var(--ct-muted-fg)]">No paired machine with Full access is online.</p>
    : null;
  return (
    <section aria-label="Where" className="flex flex-col gap-1.5 text-[12px]" data-testid="template-where">
      {needs.map((need) => need.kind === "missing_harness" ? (
        <div key={need.harness} className="flex items-center gap-2">
          <span className="text-[var(--ct-red)]">{need.harness} isn't installed on this machine</span>
          <button type="button" className="text-[var(--ct-accent)] hover:underline" onClick={() => onInstallProvider(need.harness)}>Install</button>
        </div>
      ) : <div key={need.tool} className="text-[var(--ct-red)]">{need.tool} isn't available on this machine</div>)}
      {open !== "roles" && !choice.remotePeer && workers.length > 0
        && <button type="button" className={LINK} onClick={() => reveal("roles")}>Run some roles on another machine</button>}
      {open === "roles" && (
        <div role="group" aria-label="Worker placement" className="flex flex-col gap-1.5">
          {noPeers}
          {workers.map((role) => (
            <div key={role.role} className="flex items-center justify-between gap-3">
              <span>{role.label}</span>
              <DropdownSelect className="w-44 shrink-0" aria-label={`Machine for ${role.label}`} options={options}
                value={choice.placements[role.role] ?? HOME} disabled={peers === null} onChange={(peer) => choice.setPlacement(role.role, peer)} />
            </div>
          ))}
        </div>
      )}
      {open !== "team" && <button type="button" className={LINK} onClick={() => reveal("team")}>Run this team on another machine…</button>}
      {open === "team" && (
        <div role="group" aria-label="Team placement" className="flex flex-col gap-1.5">
          {noPeers}
          <div className="flex items-center justify-between gap-3">
            <span>Run this team on</span>
            <DropdownSelect className="w-44 shrink-0" aria-label="Machine for this team" options={options}
              value={choice.remotePeer ?? HOME} disabled={peers === null} onChange={(peer) => choice.setRemotePeer(peer || null)} />
          </div>
          {choice.remotePeer && <p className="text-[11px] text-[var(--ct-muted-fg)]">
            Its managers run on {choice.remotePeer}. Nothing starts until {choice.remotePeer}'s owner taps Start here.
          </p>}
        </div>
      )}
    </section>
  );
}
