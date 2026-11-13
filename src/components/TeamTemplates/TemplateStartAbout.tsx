import { useRef, useState } from "react";
import type { ProviderDescriptor } from "../../lib/types";
import { TemplateIdentity } from "./TemplateIdentity";
import { TemplateBrings } from "./TemplateBrings";
import { TemplateAsks } from "./TemplateAsks";
import { TemplateWhere, TemplateWhereIssues } from "./TemplateWhere";
import { WorkerChips } from "./WorkerChips";
import { reviewRoles } from "./templateRoles";
import type { TeamTemplateChoice } from "./useTeamTemplateChoice";

type TemplatePreview = import("../../lib/transport/teamTemplates").TemplatePreview;

export function TemplateStartAbout({ choice, preview, providers, onInstallProvider }: {
  choice: TeamTemplateChoice;
  preview: TemplatePreview;
  providers: readonly ProviderDescriptor[];
  onInstallProvider: (providerId: string) => void;
}) {
  const details = useRef<HTMLDetailsElement>(null);
  const [placementOpen, setPlacementOpen] = useState(choice.remotePeer !== null);
  const asks = preview.asks ?? [];
  const secrets = { bindings: choice.secrets, vault: choice.vault, onBind: choice.setSecret };
  const readable = reviewRoles(preview.form.roles);
  const workers = preview.form.roles.filter((role) => role.kind === "worker").length;
  const revealPlacement = () => {
    if (details.current) details.current.open = true;
    choice.wantPeers();
    setPlacementOpen(true);
  };
  return <>
    <TemplateAsks asks={asks.filter((ask) => ask.kind === "secret")} secrets={secrets} title="Secrets" />
    <TemplateWhereIssues choice={choice} preview={preview} providers={providers} onInstallProvider={onInstallProvider} onRunElsewhere={revealPlacement} />
    <details ref={details} className="text-[12px]">
      <summary className="cursor-pointer text-[var(--ct-muted-fg)]">About this team{workers > 0 ? ` · ${workers} workers` : ""}</summary>
      <div className="mt-3 flex flex-col gap-3">
        <TemplateIdentity summary={preview.form.template} metadataOnly />
        <WorkerChips preview={preview} />
        {readable.length > 0 && <button type="button" className="self-start text-[var(--ct-accent)] hover:underline" onClick={() => choice.readInstructions(readable[0].role)}>
          Read the instructions for {readable.length === 1 ? "1 role" : `${readable.length} roles`}
        </button>}
        <TemplateBrings payload={preview.payload ?? []} optOut={choice.optOut} onToggle={choice.setKept} collapsible={false} template={preview.form.template} roles={preview.form.roles} />
        <TemplateAsks asks={asks.filter((ask) => ask.kind !== "secret")} title="Permissions and tools" />
        <TemplateWhere choice={choice} preview={preview} open={placementOpen} onOpen={revealPlacement} showLink={choice.remotePeer !== null || !(preview.where_needed?.needs.length)} />
      </div>
    </details>
  </>;
}
