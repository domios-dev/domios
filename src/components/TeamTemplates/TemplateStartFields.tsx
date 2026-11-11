import { useId } from "react";
import type { ProviderDescriptor } from "../../lib/types";
import { DropdownSelect } from "../shared/DropdownMenu";
import type { OrchestrationPairValues } from "../Sidebar/OrchestrationPairForm";
import { TemplateHeader } from "./TemplateHeader";
import { TemplateManagers, type PairRoleControls } from "./TemplateManagers";
import { TemplateBrings } from "./TemplateBrings";
import { TemplateAsks } from "./TemplateAsks";
import type { TeamTemplateChoice } from "./useTeamTemplateChoice";

type TemplatePreview = import("../../lib/transport/teamTemplates").TemplatePreview;
type TemplateInputField = import("../../../gen/TemplateInputField").TemplateInputField;

function inputLabel(field: TemplateInputField): string {
  const name = field.key.replace(/_/g, " ");
  return `${name.charAt(0).toUpperCase()}${name.slice(1)}${field.required ? "" : " · optional"}`;
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
            options={[...(field.required ? [] : [{ value: "", label: "Skip" }]), ...field.choices.map((option) => ({ value: option, label: option }))]} />
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
  const workers = preview.form.roles.filter((role) => role.group === "worker");
  const needs = preview.where_needed?.needs ?? [];
  return (
    <div className="flex flex-col gap-3" data-testid="template-start-form">
      <TemplateHeader summary={preview.form.template} onChange={choice.openGallery} onDetails={choice.showDetails} />
      {(preview.form.inputs ?? []).map((field) => (
        <InputField key={field.key} field={field} value={choice.inputs[field.key] ?? ""} onChange={(value) => choice.setInput(field.key, value)} />
      ))}
      <TemplateManagers form={preview.form} providers={providers} disabledProviderIds={disabledProviderIds} pair={pair} onPairChange={onPairChange}
        seats={choice.seats} onSeatChange={choice.setSeat} onInstallProvider={onInstallProvider} roleControls={roleControls} />
      {workers.length > 0 && (
        <div className="flex flex-wrap items-center gap-1.5 text-[11px]" aria-label="Workers on demand" role="group">
          <span className="text-[var(--ct-muted-fg)]">Workers on demand</span>
          {workers.map((role) => <span key={role.role} className="ct-small-pill">{role.label}</span>)}
        </div>
      )}
      <TemplateBrings payload={preview.payload ?? []} optOut={choice.optOut} onToggle={choice.setKept} template={preview.form.template} />
      <TemplateAsks asks={preview.asks ?? []} secrets={{ bindings: choice.secrets, vault: choice.vault, onBind: choice.setSecret }} />
      {needs.length > 0 && (
        <section aria-label="Where" className="flex flex-col gap-1 text-[12px]" data-testid="template-where">
          {needs.map((need) => need.kind === "missing_harness" ? (
            <div key={need.harness} className="flex items-center gap-2">
              <span className="text-[var(--ct-red)]">{need.harness} isn't installed on this machine</span>
              <button type="button" className="text-[var(--ct-accent)] hover:underline" onClick={() => onInstallProvider(need.harness)}>Install</button>
            </div>
          ) : <div key={need.tool} className="text-[var(--ct-red)]">{need.tool} isn't available on this machine</div>)}
        </section>
      )}
    </div>
  );
}
