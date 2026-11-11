import { useId } from "react";
import { BackPill } from "../shared/BackPill";
import { Button } from "../shared/Button";
import { ConsentLayout } from "../shared/ConsentLayout";
import { managerLabel, roleTiming, sourceLabel, templateTitle } from "../../lib/teamTemplates/startModel";
import { TemplateIcon } from "./TemplateIcon";
import { TemplateBrings } from "./TemplateBrings";
import { TemplateAsks } from "./TemplateAsks";
import type { TeamTemplateChoice } from "./useTeamTemplateChoice";
import { authorRows, harnessRequirements, linkRows, updateDiffLines } from "../../lib/teamTemplates/exchangeModel";

type TemplatePreview = import("../../lib/transport/teamTemplates").TemplatePreview;
type TemplateRoleSummary = import("../../../gen/TemplateRoleSummary").TemplateRoleSummary;
type TemplateSummary = import("../../../gen/TemplateSummary").TemplateSummary;
type TemplateUpdateDiff = import("../../../gen/TemplateUpdateDiff").TemplateUpdateDiff;

const LINK = "text-[var(--ct-accent)] hover:underline";

function Identity({ summary }: { summary: TemplateSummary }) {
  const authors = authorRows(summary);
  const links = linkRows(summary);
  if (!authors.length && !summary.license && !links.length) return null;
  return (
    <div className="flex flex-col gap-1 text-[12px] text-[var(--ct-muted-fg)]" data-testid="template-identity">
      {authors.length > 0 && <div>{authors.map((author, index) => <span key={`${author.name}:${index}`}>
        {index > 0 && ", "}
        {author.url ? <a href={author.url} target="_blank" rel="noreferrer noopener" className={LINK}>{author.name}</a> : author.name}
        {author.role && ` · ${author.role}`}
      </span>)}</div>}
      {summary.license && <div>License <span className="font-mono">{summary.license}</span></div>}
      {links.length > 0 && <div className="flex flex-wrap gap-3">{links.map((link) =>
        <a key={link.label} href={link.url} target="_blank" rel="noreferrer noopener" className={LINK}>{link.label}</a>)}</div>}
    </div>
  );
}

function UpdateDiff({ diff }: { diff: TemplateUpdateDiff }) {
  const lines = updateDiffLines(diff);
  return (
    <div className="ct-card ct-card--static flex flex-col gap-1 p-3 text-[12px]" role="status" data-testid="template-update-diff">
      <div className="font-medium">Updates {diff.name} from {diff.from_version} to {diff.to_version}</div>
      {lines.length ? <ul className="flex flex-col gap-0.5 text-[var(--ct-muted-fg)]">{lines.map((line) => <li key={line}>{line}</li>)}</ul>
        : <div className="text-[var(--ct-muted-fg)]">Same roles, items and asks.</div>}
      <div className="text-[11px] text-[var(--ct-muted-fg)]">Teams already started keep the version they started with.</div>
    </div>
  );
}

function ModelCapsule({ preview, role }: { preview: TemplatePreview; role: TemplateRoleSummary }) {
  const decision = preview.form.pickers.find((picker) => picker.slot === role.role && role.group === "manager")?.decision;
  if (!decision) return null;
  const text = decision.reason === "same_as" ? "Same as Orchestrator" : decision.choice?.model ?? "You pick";
  return <span className={`shrink-0 rounded-full px-2 py-0.5 font-mono text-[10px] ${decision.choice ? "bg-[var(--ct-hover)]" : "border border-dashed border-[var(--ct-border)] text-[var(--ct-muted-fg)]"}`}>{text}</span>;
}

function RoleList({ title, roles, preview }: { title: string; roles: TemplateRoleSummary[]; preview: TemplatePreview }) {
  if (roles.length === 0) return null;
  return (
    <section aria-label={title} className="flex flex-col gap-2">
      <div className="text-[11px] text-[var(--ct-muted-fg)]">{title}</div>
      {roles.map((role) => (
        <div key={`${role.group}:${role.role}`} className="flex items-start gap-2 text-[12px]">
          <div className="min-w-0 flex-1">
            <div>{role.group === "manager" ? managerLabel(role.role, role.label) : role.label}</div>
            <div className="text-[11px] text-[var(--ct-muted-fg)]">{role.description ?? roleTiming(role.start) ?? "Starts with the team"}</div>
          </div>
          <ModelCapsule preview={preview} role={role} />
        </div>
      ))}
    </section>
  );
}

/** A package's identity, roles, payload and asks. Consent mode lets the owner opt out before using it; detail mode is read-only. */
export function TemplateReview({ choice, preview, mode, onExport, onInstallProvider }: {
  choice: TeamTemplateChoice;
  preview: TemplatePreview;
  mode: "consent" | "detail";
  onExport: () => void;
  onInstallProvider: (providerId: string) => void;
}) {
  const titleId = useId();
  const descriptionId = useId();
  const summary = preview.form.template;
  const missing = preview.where_needed?.needs ?? [];
  const missingHarnesses = new Set(missing.flatMap((need) => need.kind === "missing_harness" ? [need.harness] : []));
  const required = harnessRequirements(preview.requires_harnesses ?? [], missingHarnesses);
  const update = choice.update && choice.update.name === summary.name && choice.update.to_version === summary.version ? choice.update : null;
  const consent = mode === "consent";
  return (
    <section aria-labelledby={titleId} aria-describedby={descriptionId} className="flex flex-col gap-3" data-testid={`template-review-${mode}`}>
      <div><BackPill label={consent ? "templates" : "the Team form"} onBack={consent ? choice.openGallery : choice.backToForm} /></div>
      <ConsentLayout titleId={titleId} descriptionId={descriptionId}
        title={<span className="flex items-center gap-3">
          <TemplateIcon icon={summary.icon} size={44} />
          <span className="min-w-0">
            <span className="flex items-baseline gap-2">{templateTitle(summary)}<span className="font-mono text-[11px] font-normal text-[var(--ct-muted-fg)]">{summary.version}</span></span>
            <span className="block text-[11px] font-normal text-[var(--ct-muted-fg)]">{sourceLabel(summary.provenance)}{summary.derived_from && ` · based on ${summary.derived_from.name} ${summary.derived_from.version}`}</span>
          </span>
        </span>}
        description={consent && preview.consent?.reasons.includes("unverified_package")
          ? "This team was shared with you. Check what it brings and turn off anything you don't want; nothing runs until you start the team."
          : summary.description}
        footnote={(required.length > 0 || missing.length > 0) && <span data-testid="template-requires">
          {required.length > 0 && required.every((need) => need.installed)
            && <span className="mr-2">Needs {required.map((need) => need.harness).join(" and ")} · {required.length > 1 ? "all installed" : "installed"}</span>}
          {required.filter((need) => !need.installed).map((need) => <span key={need.harness} className="mr-2 text-[var(--ct-red)]">
            Needs {need.harness} · not installed <button type="button" className={LINK} onClick={() => onInstallProvider(need.harness)}>Install</button></span>)}
          {missing.map((need) => need.kind === "missing_harness"
            ? required.some((entry) => entry.harness === need.harness) ? null
              : <span key={need.harness} className="mr-2 text-[var(--ct-red)]">Needs {need.harness} · not installed <button type="button" className={LINK} onClick={() => onInstallProvider(need.harness)}>Install</button></span>
            : <span key={need.tool} className="mr-2 text-[var(--ct-red)]">Needs {need.tool}</span>)}
        </span>}
        actions={consent ? <>
          <Button type="button" variant="ghost" onClick={choice.openGallery}>Not now</Button>
          <Button type="button" variant="primary" onClick={choice.acceptReviewed}>Use template</Button>
        </> : <>
          {summary.provenance === "saved" && <Button type="button" variant="ghost" onClick={onExport}>Export…</Button>}
          <Button type="button" variant="primary" onClick={choice.acceptReviewed}>Use template</Button>
        </>}>
        {consent && <p className="text-[12px] text-[var(--ct-muted-fg)]">{summary.description}</p>}
        <Identity summary={summary} />
        {update && <UpdateDiff diff={update} />}
        <div className="grid gap-4 sm:grid-cols-2">
          <div className="flex flex-col gap-3">
            <RoleList title="Managers" roles={preview.form.roles.filter((role) => role.group === "manager")} preview={preview} />
            <RoleList title="Workers on demand" roles={preview.form.roles.filter((role) => role.group === "worker")} preview={preview} />
          </div>
          <div className="flex flex-col gap-3">
            <TemplateBrings payload={preview.payload ?? []} optOut={consent ? choice.optOut : new Set()} onToggle={consent ? choice.setKept : undefined} collapsible={false} />
            <TemplateAsks asks={preview.asks ?? []} />
          </div>
        </div>
      </ConsentLayout>
    </section>
  );
}
