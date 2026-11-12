import { useId, type ReactNode } from "react";
import "./teamTemplates.css";
import { BookOpen, Bug, Eye, GitBranch, Home, Users } from "lucide-react";
import type { ProviderDescriptor } from "../../lib/types";
import { AgentIcon } from "../AgentIcon";
import { BackPill } from "../shared/BackPill";
import { Button } from "../shared/Button";
import { kindDisplayName } from "../../lib/roleKind";
import {
  effortLabel, humanizePackageName, managerDecision, managerRoles, roleSameAs, roleTiming, splitModel, templateSource, templateTitle,
  visibleAsks, visiblePayload, type TemplateSummary,
} from "../../lib/teamTemplates/startModel";
import { TemplateIcon } from "./TemplateIcon";
import { TemplateTrust } from "./TemplateTrust";
import { TemplateBrings } from "./TemplateBrings";
import { TemplateAsks } from "./TemplateAsks";
import { SLOT_ICON } from "./TemplateManagers";
import { reviewRoles, roleHeading } from "./templateRoles";
import type { TeamTemplateChoice } from "./useTeamTemplateChoice";
import { authorRows, harnessRequirements, linkRows, updateDiffLines } from "../../lib/teamTemplates/exchangeModel";

type TemplatePreview = import("../../lib/transport/teamTemplates").TemplatePreview;
type TemplateRoleSummary = import("../../../gen/TemplateRoleSummary").TemplateRoleSummary;
type TemplateUpdateDiff = import("../../../gen/TemplateUpdateDiff").TemplateUpdateDiff;
type ManagerSlot = import("../../../gen/ManagerSlot").ManagerSlot;

const LINK = "inline-flex items-center gap-1 text-[var(--ct-accent)] hover:underline";
const LINK_ICON: Record<string, ReactNode> = {
  Homepage: <Home size={12} aria-hidden="true" />, Repository: <GitBranch size={12} aria-hidden="true" />,
  Issues: <Bug size={12} aria-hidden="true" />, Docs: <BookOpen size={12} aria-hidden="true" />,
};

function Identity({ summary, titleId, descriptionId }: { summary: TemplateSummary; titleId: string; descriptionId: string }) {
  const authors = authorRows(summary);
  const links = linkRows(summary);
  const lineage = summary.derived_from ? `based on ${humanizePackageName(summary.derived_from.name)} ${summary.derived_from.version}` : null;
  const facts = [summary.license, lineage].filter(Boolean).join(" · ");
  return (
    <div className="flex gap-3.5" data-testid="template-identity">
      <TemplateIcon icon={summary.icon} size={44} />
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-baseline gap-x-2 gap-y-0.5">
          <h2 id={titleId} className="text-[15px] font-semibold">{templateTitle(summary)}</h2>
          {summary.version && <span className="font-mono text-[11px] text-[var(--ct-muted-fg)]">{summary.version}</span>}
          <TemplateTrust summary={summary} />
        </div>
        <p id={descriptionId} className="mt-0.5 text-[12.5px] leading-snug text-[color-mix(in_srgb,var(--ct-fg)_85%,transparent)]">{summary.description}</p>
        {(authors.length > 0 || facts) && <div className="mt-1.5 flex flex-wrap gap-x-3.5 gap-y-1 text-[11.5px] text-[var(--ct-muted-fg)]">
          {authors.map((author, index) => <span key={`${author.name}:${index}`}>
            {author.url ? <a href={author.url} target="_blank" rel="noreferrer noopener" className={LINK}>{author.name}</a> : author.name}
            {author.role && ` · ${author.role}`}
          </span>)}
          {facts && <span>{facts}</span>}
        </div>}
        {links.length > 0 && <div className="mt-1 flex flex-wrap gap-x-3.5 gap-y-1 text-[11.5px]">{links.map((link) =>
          <a key={link.label} href={link.url} target="_blank" rel="noreferrer noopener" className={LINK}>{LINK_ICON[link.label]}{link.label}</a>)}</div>}
      </div>
    </div>
  );
}

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

const CAPSULE = "ml-auto inline-flex shrink-0 items-center gap-1.5 whitespace-nowrap rounded-full px-2 py-0.5 text-[10.5px]";
const UNSET = `${CAPSULE} border border-dashed border-[color-mix(in_srgb,var(--ct-fg)_18%,transparent)] text-[var(--ct-muted-fg)]`;

function ModelCapsule({ preview, role, providers }: { preview: TemplatePreview; role: TemplateRoleSummary; providers: readonly ProviderDescriptor[] }) {
  const decision = role.group === "manager" ? managerDecision(preview.form.pickers, role) : role.model;
  const target = roleSameAs(role, decision);
  if (target) return <span className={UNSET}>Same as {kindDisplayName(target)}</span>;
  if (!decision?.choice) return <span className={UNSET}>You pick</span>;
  const { providerId, modelId } = splitModel(decision.choice.model);
  const model = providers.find((provider) => provider.id === providerId)?.available_models.find((entry) => entry.id === modelId)?.display_name ?? modelId;
  const effort = decision.effort?.applied;
  return (
    <span className={`${CAPSULE} bg-[color-mix(in_srgb,var(--ct-fg)_6%,transparent)] font-mono`}>
      <AgentIcon providerId={providerId} className="text-[11px]" />{effort ? `${model} · ${effortLabel(effort)}` : model}
    </span>
  );
}

function RoleList({ title, roles, preview, providers }: { title: string; roles: TemplateRoleSummary[]; preview: TemplatePreview; providers: readonly ProviderDescriptor[] }) {
  if (roles.length === 0) return null;
  return (
    <section aria-label={title} className="flex flex-col gap-0.5">
      <div className="text-[11px] font-medium text-[var(--ct-muted-fg)]">{title}</div>
      {roles.map((role) => {
        const heading = roleHeading(role);
        const start = role.group === "manager"
          ? roleTiming(role.start) ?? "Starts with the team"
          : role.description ?? roleTiming(role.start) ?? "Starts with the team";
        return (
          <div key={`${role.group}:${role.role}`} className="flex items-center gap-2.5 py-1 text-[12px]">
            <span aria-hidden="true" className="grid h-[22px] w-[22px] shrink-0 place-items-center rounded-[7px] bg-[color-mix(in_srgb,var(--ct-accent)_12%,transparent)] text-[var(--ct-accent)]">
              {role.group === "manager" ? SLOT_ICON[role.role as ManagerSlot] ?? <Users size={13} /> : <Users size={13} />}
            </span>
            <div className="min-w-0 flex-1">
              <div>{heading.name}{heading.slot && <span className="text-[var(--ct-muted-fg)]"> · {heading.slot}</span>}</div>
              <div className="text-[11.5px] text-[var(--ct-muted-fg)]">{start}</div>
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
        <Button type="button" variant="ghost" className="text-xs" onClick={() => onInstallProvider(harness)}>Install</Button>
      </span>)}
      {tools.map((tool) => <span key={tool} className="text-[var(--ct-fg)]">Needs {tool}</span>)}
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
  const descriptionId = useId();
  const summary: TemplateSummary = preview.form.template;
  const source = templateSource(summary);
  const update = choice.update && choice.update.name === summary.name && choice.update.to_version === summary.version ? choice.update : null;
  const consent = mode === "consent";
  const back = choice.view.kind === "review" ? choice.view.back : "gallery";
  const managers = managerRoles(preview.form.roles);
  const workers = preview.form.roles.filter((role) => role.group === "worker");
  const readable = reviewRoles(preview.form.roles);
  const unverified = consent && preview.consent?.reasons.includes("unverified_package");
  const twoColumns = visiblePayload(preview.payload ?? []).length > 0 || visibleAsks(preview.asks ?? []).length > 0;
  return (
    <section aria-labelledby={titleId} aria-describedby={descriptionId} className="flex flex-col gap-3" data-testid={`template-review-${mode}`}>
      <div><BackPill label={back === "form" ? "the Team form" : "templates"} onBack={choice.backFromReview} /></div>
      <Identity summary={summary} titleId={titleId} descriptionId={descriptionId} />
      {unverified && <p className="text-[11.5px] text-[var(--ct-muted-fg)]">
        {summary.received_from?.trim() ? `${summary.received_from.trim()} shared this team` : "This team came from a file"}. Check what it brings and turn off anything you don't want. Nothing runs until you start the team.
      </p>}
      {update && <UpdateDiff diff={update} />}
      <div className={`grid gap-5 ${twoColumns ? "sm:grid-cols-2" : ""}`} data-testid="template-review-body" data-columns={twoColumns ? 2 : 1}>
        <div className="flex flex-col gap-3">
          <RoleList title="Managers" roles={managers} preview={preview} providers={providers} />
          <RoleList title="Workers on demand" roles={workers} preview={preview} providers={providers} />
          {readable.length > 0 && <button type="button" className={`self-start text-[11.5px] ${LINK}`} onClick={() => choice.readInstructions(readable[0].role)}>
            <Eye size={12} aria-hidden="true" />Read the instructions for {readable.length === 1 ? "1 role" : `${readable.length} roles`}
          </button>}
        </div>
        {twoColumns && <div className="flex flex-col gap-3">
          <TemplateBrings payload={preview.payload ?? []} optOut={consent ? choice.optOut : new Set()} onToggle={consent ? choice.setKept : undefined}
            collapsible={false} template={summary} roles={preview.form.roles} />
          <TemplateAsks asks={preview.asks ?? []} />
        </div>}
      </div>
      {/* One scroll container: the dialog scrolls and the footer stays pinned at its bottom edge. */}
      <div className="template-pinned-footer sticky z-[1] flex flex-col gap-2 border-t border-[var(--ct-border)] bg-[var(--ct-surface)] text-[11.5px] text-[var(--ct-muted-fg)] sm:flex-row sm:items-center">
        <span aria-hidden="true" className="pointer-events-none absolute inset-x-0 -top-6 h-6 bg-gradient-to-t from-[var(--ct-surface)] to-transparent" />
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <HarnessNote preview={preview} providers={providers} onInstallProvider={onInstallProvider} />
          {/* Use only picks the template; the start is what the owner confirms, and only then is it verified. */}
          {consent && <span data-testid="template-consent-note">You'll confirm when you start.</span>}
        </div>
        <div className="flex flex-col-reverse gap-2 sm:flex-row">
          {consent
            ? <Button type="button" variant="ghost" className="w-full sm:w-auto" onClick={choice.openGallery}>Not now</Button>
            : source.exportable && <Button type="button" variant="ghost" className="w-full sm:w-auto" onClick={onExport}>Export…</Button>}
          <Button type="button" variant="primary" className="w-full sm:w-auto" onClick={choice.acceptReviewed}>Use template</Button>
        </div>
      </div>
    </section>
  );
}
