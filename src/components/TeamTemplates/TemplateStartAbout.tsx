import { useState } from "react";
import type { ProviderDescriptor } from "../../lib/types";
import { TemplateAboutDialog } from "./TemplateAboutDialog";
import { TemplateAsks } from "./TemplateAsks";
import { TemplateWhere, TemplateWhereIssues } from "./TemplateWhere";
import type { TeamTemplateChoice } from "./useTeamTemplateChoice";

type TemplatePreview = import("../../lib/transport/teamTemplates").TemplatePreview;

export function TemplateStartAbout({ choice, preview, providers, onInstallProvider }: {
  choice: TeamTemplateChoice;
  preview: TemplatePreview;
  providers: readonly ProviderDescriptor[];
  onInstallProvider: (providerId: string) => void;
}) {
  const [aboutOpen, setAboutOpen] = useState(false);
  const [placementOpen, setPlacementOpen] = useState(choice.remotePeer !== null);
  const workers = preview.form.roles.filter((role) => role.kind === "worker").length;
  const revealPlacement = () => {
    choice.wantPeers();
    setPlacementOpen(true);
  };
  return <>
    <TemplateAsks asks={(preview.asks ?? []).filter((ask) => ask.kind === "secret")} secrets={{ bindings: choice.secrets, vault: choice.vault, onBind: choice.setSecret, onAddSecret: () => choice.openSettings("secrets") }} title="Secrets" />
    <TemplateWhereIssues choice={choice} preview={preview} providers={providers} onInstallProvider={onInstallProvider} onRunElsewhere={revealPlacement} />
    <TemplateWhere choice={choice} preview={preview} open={placementOpen} onOpen={revealPlacement} showLink={choice.remotePeer !== null || !(preview.where_needed?.needs.length)} />
    <button type="button" className="self-start text-[12px] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]" onClick={() => setAboutOpen(true)}>
      About this team{workers > 0 ? ` · ${workers} ${workers === 1 ? "worker" : "workers"}` : ""}
    </button>
    {aboutOpen && <TemplateAboutDialog choice={choice} preview={preview} providers={providers} onClose={() => setAboutOpen(false)} />}
  </>;
}
