import { useId, useState } from "react";
import "./teamTemplates.css";
import { Users } from "lucide-react";
import type { ProviderDescriptor } from "../../lib/types";
import { BackPill } from "../shared/BackPill";
import { Button } from "../shared/Button";
import {
  humanizePackageName, lacksAuthority, managerRoles, roleTiming, templateSource,
  type TemplateSummary,
} from "../../lib/teamTemplates/startModel";
import { TemplateIdentity } from "./TemplateIdentity";
import { TemplateBrings } from "./TemplateBrings";
import { TemplateAsks } from "./TemplateAsks";
import { TemplateAboutDialog } from "./TemplateAboutDialog";
import { ReadInstructionsButton } from "./ReadInstructionsButton";
import { ModelCapsule } from "./TemplateRoleModel";
import { PinnedFooter } from "./PinnedFooter";
import { managerIcon } from "./TemplateManagers";
import { roleHeading } from "./templateRoles";
import type { TeamTemplateChoice } from "./useTeamTemplateChoice";
import { harnessRequirements, updateDiffLines } from "../../lib/teamTemplates/exchangeModel";

type TemplatePreview = import("../../lib/transport/teamTemplates").TemplatePreview;
type TemplateRoleSummary = import("../../../gen/TemplateRoleSummary").TemplateRoleSummary;
type TemplateUpdateDiff = import("../../../gen/TemplateUpdateDiff").TemplateUpdateDiff;

function UpdateDiff({ diff }: { diff: TemplateUpdateDiff }) {
  const lines = updateDiffLines(diff);
  return (
    <div className="ct-card ct-card--static flex flex-col gap-1 p-3 text-[12px]" role="status" data-testid="template-update-diff">
      <div className="font-medium">Updates {humanizePackageName(diff.name)} from {diff.from_version} to {diff.to_version}</div>
      {lines.length ? <ul className="flex flex-col gap-0.5 text-[var(--ct-muted-fg)]">{lines.map((line) => <li key={line}>{line}</li>)}</ul>
        : <div className="text-[var(--ct-muted-fg)]">Same roles, items and asks.</div>}
      <div className="text-[11px] text-[var(--ct-muted-fg)]">Teams already started keep the version they started with.</div>
    </div>
  );
}

function RoleList({ title, roles, preview, providers }: { title: string; roles: TemplateRoleSummary[]; preview: TemplatePreview; providers: readonly ProviderDescriptor[] }) {
  if (roles.length === 0) return null;
  return (
    <section aria-label={title} className="flex flex-col gap-0.5">
      <div className="text-[11px] font-medium text-[var(--ct-muted-fg)]">{title}</div>
      {roles.map((role) => {
        const heading = roleHeading(role);
        const start = [role.description?.trim(), roleTiming(role.start) ?? "Starts with the team"].filter(Boolean).join(" · ");
        return (
          <div key={`${role.kind}:${role.role}`} className="flex flex-wrap items-center gap-2.5 py-1 text-[12px]" data-role-kind={role.kind}>
            <span aria-hidden="true" className="grid h-[22px] w-[22px] shrink-0 place-items-center rounded-[7px] bg-[color-mix(in_srgb,var(--ct-accent)_12%,transparent)] text-[var(--ct-accent)]">
              {role.kind === "manager" ? managerIcon(role.role) : <Users size={13} />}
            </span>
            <div className="min-w-0 flex-1">
              <div>{heading.name}{heading.slot && <span className="text-[var(--ct-muted-fg)]"> · {heading.slot}</span>}</div>
              <div className="text-[11.5px] text-[var(--ct-muted-fg)]">{start}</div>
              {lacksAuthority(role) && <div className="text-[11px] text-[var(--ct-muted-fg)]" data-authority="none">No Domios authority</div>}
            </div>
            <ModelCapsule preview={preview} role={role} providers={providers} />
          </div>
        );
      })}
    </section>
  );
}

function HarnessNote({ preview, providers, onInstallProvider }: { preview: TemplatePreview; providers: readonly ProviderDescriptor[]; onInstallProvider: (id: string) => void }) {
  const name = (id: string) => providers.find((provider) => provider.id === id)?.display_name ?? id;
  const missing = preview.where_needed?.needs ?? [];
  const missingHarnesses = new Set(missing.flatMap((need) => need.kind === "missing_harness" ? [need.harness] : []));
  const required = harnessRequirements(preview.requires_harnesses ?? [], missingHarnesses);
  const absent = [...new Set([...required.filter((need) => !need.installed).map((need) => need.harness), ...missingHarnesses])];
  const tools = missing.flatMap((need) => need.kind === "missing_tool" ? [need.tool] : []);
  if (required.length === 0 && absent.length === 0 && tools.length === 0) return null;
  const installed = required.filter((need) => need.installed).map((need) => name(need.harness));
  return (
    <span data-testid="template-requires" className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1">
      {absent.length === 0 && installed.length > 0
        && <span>Needs {installed.join(" and ")} · {installed.length === 1 ? "installed" : installed.length === 2 ? "both installed" : "all installed"}</span>}
      {absent.map((harness) => <span key={harness} className="inline-flex items-center gap-1.5">
        <span aria-hidden="true" className="h-1.5 w-1.5 rounded-full bg-[var(--ct-red)]" />
        <span className="text-[var(--ct-fg)]">Needs {name(harness)} · not installed</span>
        <Button type="button" variant="outline" className="text-xs" onClick={() => onInstallProvider(harness)}>Install</Button>
      </span>)}
      {tools.map((tool) => <span key={tool} className="text-[var(--ct-fg)]">Needs {tool} · not available on this machine</span>)}
    </span>
  );
}

/** A package's identity, roles, payload and asks. Consent mode lets the owner opt out before using it; detail mode is read-only. */
export function TemplateReview({ choice, preview, mode, providers, onExport, onInstallProvider }: {
  choice: TeamTemplateChoice;
  preview: TemplatePreview;
  mode: "consent" | "detail";
  providers: readonly ProviderDescriptor[];
  onExport: () => void;
  onInstallProvider: (providerId: string) => void;
}) {
  const titleId = useId();
  const [reading, setReading] = useState(false);
  const descriptionId = useId();
  const summary: TemplateSummary = preview.form.template;
  const source = templateSource(summary);
  const update = choice.update && choice.update.name === summary.name && choice.update.to_version === summary.version ? choice.update : null;
  const consent = mode === "consent";
  const back = choice.view.kind === "review" ? choice.view.back : "gallery";
  const managers = managerRoles(preview.form.roles);
  const workers = preview.form.roles.filter((role) => role.kind === "worker");
  const unverified = consent && preview.consent?.reasons.includes("unverified_package");
  return (
    <section aria-labelledby={titleId} aria-describedby={descriptionId} className="flex flex-col gap-3" data-testid={`template-review-${mode}`}>
      <div><BackPill label={back === "form" ? "the Team form" : "templates"} onBack={choice.backFromReview} /></div>
      <TemplateIdentity summary={summary} titleId={titleId} descriptionId={descriptionId} />
      {unverified && <p className="text-[11.5px] text-[var(--ct-muted-fg)]">
        {summary.received_from?.trim() ? `${summary.received_from.trim()} shared this team` : "This team came from a file"}. Check what it brings and turn off anything you don't want.
      </p>}
      {update && <UpdateDiff diff={update} />}
      <div className="flex flex-col gap-4" data-testid="template-review-body">
        <div className="flex flex-col gap-3">
          <RoleList title="Managers" roles={managers} preview={preview} providers={providers} />
          <RoleList title="Workers on demand" roles={workers} preview={preview} providers={providers} />
          <ReadInstructionsButton preview={preview} onClick={() => setReading(true)} />
        </div>
        <div className="flex flex-col gap-3">
          <TemplateBrings payload={preview.payload ?? []} optOut={consent ? choice.optOut : new Set()} onToggle={consent ? choice.setKept : undefined}
            collapsible={false} template={summary} roles={preview.form.roles} />
          <TemplateAsks asks={preview.asks ?? []} />
        </div>
      </div>
      {/* One scroll container: the dialog scrolls and the footer stays pinned at its bottom edge. */}
      {/* The section's gap-3 precedes the sentinel; -13px cancels it and the sentinel's own pixel. */}
      <PinnedFooter sentinelClassName="-mt-[13px]"
        className="flex flex-col gap-2 border-t border-[var(--ct-border)] bg-[var(--ct-surface)] text-[11.5px] text-[var(--ct-muted-fg)]">
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <HarnessNote preview={preview} providers={providers} onInstallProvider={onInstallProvider} />
          {/* Use only picks the template; the start is what the owner confirms, and only then is it verified. */}
          {consent && <span data-testid="template-consent-note">You'll confirm when you start.</span>}
        </div>
        <div className="flex flex-col-reverse gap-2">
          {consent
            ? <Button type="button" variant="ghost" className="w-full" onClick={choice.openGallery}>Not now</Button>
            : source.exportable && <Button type="button" variant="ghost" className="w-full" onClick={onExport}>Export…</Button>}
          <Button type="button" variant="primary" className="w-full" onClick={choice.acceptReviewed}>Use template</Button>
        </div>
      </PinnedFooter>
      {reading && <TemplateAboutDialog choice={choice} preview={preview} providers={providers} initialSection="roles" onClose={() => setReading(false)} />}
    </section>
  );
}
