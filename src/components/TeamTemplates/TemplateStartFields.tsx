import { useId } from "react";
import type { ProviderDescriptor } from "../../lib/types";
import { humanizeRoleKey, roleDisplayLabel } from "../../lib/roleKind";
import { DropdownSelect } from "../shared/DropdownMenu";
import { inputFieldLabel } from "../../lib/teamTemplates/startModel";
import type { OrchestrationPairValues } from "../Sidebar/OrchestrationPairForm";
import { TemplateHeader } from "./TemplateHeader";
import { TemplateManagers, type PairRoleControls } from "./TemplateManagers";
import { TemplateBrings } from "./TemplateBrings";
import { TemplateAsks } from "./TemplateAsks";
import { TemplateWhere } from "./TemplateWhere";
import type { TeamTemplateChoice } from "./useTeamTemplateChoice";

type TemplatePreview = import("../../lib/transport/teamTemplates").TemplatePreview;
type TemplateInputField = import("../../../gen/TemplateInputField").TemplateInputField;

function inputLabel(field: TemplateInputField): string {
  return `${inputFieldLabel(field.key)}${field.required ? "" : " · optional"}`;
}

function InputField({ field, value, onChange }: { field: TemplateInputField; value: string; onChange: (value: string) => void }) {
  const id = useId();
  const label = inputLabel(field);
  if (field.choices?.length) {
    return (
      <div className="flex items-center justify-between gap-3">
        <span className="text-[11px] text-[var(--ct-muted-fg)]">{label}</span>
        <div className="w-40">
          <DropdownSelect aria-label={label} value={value} onChange={onChange}
            options={[...(field.required ? [] : [{ value: "", label: "Skip" }]), ...field.choices.map((option) => ({ value: option, label: humanizeRoleKey(option) }))]} />
        </div>
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-1">
      <label htmlFor={id} className="text-[11px] text-[var(--ct-muted-fg)]">{label}</label>
      <input id={id} value={value} placeholder={field.description} required={field.required} onChange={(event) => onChange(event.target.value)}
        className="rounded-full border border-[var(--ct-border)] bg-[var(--ct-bg)] px-3 py-1.5 text-[12px] outline-none" />
    </div>
  );
}

/** The workers a template brings on demand; nothing is spawned at start. */
export function WorkerChips({ preview }: { preview: TemplatePreview }) {
  const workers = preview.form.roles.filter((role) => role.kind === "worker");
  if (workers.length === 0) return null;
  return (
    <div className="flex flex-wrap items-center gap-1.5 text-[11px]" aria-label="Workers on demand" role="group">
      <span className="text-[var(--ct-muted-fg)]">Workers on demand</span>
      {workers.map((role) => <span key={role.role} className="ct-small-pill" title={role.description ?? undefined}>{roleDisplayLabel(role.role, role.label)}</span>)}
    </div>
  );
}

/** The simple start form for a chosen template, in the order the owner fills it. */
export function TemplateStartFields({ choice, preview, providers, disabledProviderIds, pair, onPairChange, onInstallProvider, roleControls }: {
  choice: TeamTemplateChoice;
  preview: TemplatePreview;
  providers: ProviderDescriptor[];
  disabledProviderIds: ReadonlySet<string>;
  pair: OrchestrationPairValues;
  onPairChange: (next: OrchestrationPairValues) => void;
  onInstallProvider: (providerId: string) => void;
  roleControls?: PairRoleControls;
}) {
  return (
    <div className="flex flex-col gap-3" data-testid="template-start-form">
      <TemplateHeader summary={preview.form.template} onChange={choice.openGallery} onDetails={choice.showDetails} />
      {(preview.form.inputs ?? []).map((field) => (
        <InputField key={field.key} field={field} value={choice.inputs[field.key] ?? ""} onChange={(value) => choice.setInput(field.key, value)} />
      ))}
      <TemplateManagers form={preview.form} providers={providers} disabledProviderIds={disabledProviderIds} pair={pair} onPairChange={onPairChange}
        seats={choice.seats} onSeatChange={choice.setSeat} onSeatClear={choice.clearSeat} onInstallProvider={onInstallProvider} roleControls={roleControls} />
      <WorkerChips preview={preview} />
      <TemplateBrings payload={preview.payload ?? []} optOut={choice.optOut} onToggle={choice.setKept} template={preview.form.template} roles={preview.form.roles} />
      <TemplateAsks asks={preview.asks ?? []} secrets={{ bindings: choice.secrets, vault: choice.vault, onBind: choice.setSecret }} />
      <TemplateWhere choice={choice} preview={preview} providers={providers} onInstallProvider={onInstallProvider} />
    </div>
  );
}
