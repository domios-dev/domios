import { useId, useState } from "react";
import { FormDialog } from "../shared/FormDialog";
import { Button } from "../shared/Button";
import { teamTemplatesTransport } from "../../lib/transport/orchestrationRuntime";
import type { TeamTemplatesTransport, TemplateExportResult } from "../../lib/transport/teamTemplates";
import {
  DEFAULT_EXPORT_FORMAT, exportFormats, scrubRows, startErrorView, templateTitle,
  type PackageFormat, type ScrubReport, type TemplateSummary,
} from "../../lib/teamTemplates/startModel";

export function ScrubReportCard({ report }: { report: ScrubReport }) {
  return (
    <ul className="ct-card ct-card--static flex flex-col gap-1.5 p-3" aria-label="Removed before saving">
      {scrubRows(report).map((row) => (
        <li key={row.label} className="flex items-center gap-2 text-[12px]">
          <span>{row.label}</span>
          <span className="min-w-0 flex-1 truncate text-[var(--ct-muted-fg)]">{row.detail}</span>
          <span className="ct-small-pill font-mono" aria-label={`${row.count} ${row.label.toLowerCase()}`}>{row.count}</span>
        </li>
      ))}
    </ul>
  );
}

export function TemplateExportDialog({ summary, onClose, transport = teamTemplatesTransport }: {
  summary: TemplateSummary;
  onClose: () => void;
  transport?: () => TeamTemplatesTransport;
}) {
  const titleId = useId();
  const descriptionId = useId();
  const options = exportFormats(summary);
  const [format, setFormat] = useState<PackageFormat>(DEFAULT_EXPORT_FORMAT);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<TemplateExportResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      setResult(await transport().export({ template: { name: summary.name, version: summary.version }, format }));
    } catch (failure) {
      setError(startErrorView(failure).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <FormDialog titleId={titleId} descriptionId={descriptionId} className="max-w-md" onCancel={onClose}
      onSubmit={(event) => { event.preventDefault(); if (result) onClose(); else void submit(); }}>
      <h2 id={titleId} className="text-base font-semibold">Export {templateTitle(summary)}</h2>
      <p id={descriptionId} className="text-sm text-[var(--ct-muted)]">Machines, paths and secret values are removed from every export.</p>
      {result ? <>
        <ScrubReportCard report={result.scrub} />
        {!result.check.valid && <ul role="alert" className="text-[12px] text-[var(--ct-red)]">
          {result.check.diagnostics.map((diagnostic) => <li key={`${diagnostic.file}:${diagnostic.path}:${diagnostic.code}`}>{diagnostic.message}</li>)}
        </ul>}
      </> : (
        <fieldset className="flex flex-col gap-1.5">
          <legend className="mb-1 text-[11px] text-[var(--ct-muted-fg)]">Format</legend>
          {options.map((option) => (
            <label key={option.format} className={`ct-card flex items-start gap-2.5 p-2.5 ${option.disabledReason ? "opacity-55" : ""}`}>
              <input type="radio" name="template-format" value={option.format} checked={format === option.format}
                disabled={Boolean(option.disabledReason)} onChange={() => setFormat(option.format)} className="mt-0.5" />
              <span className="min-w-0 flex-1 text-[12px]">
                <span className="flex flex-wrap items-baseline gap-2"><span className="font-medium">{option.name}</span><span className="font-mono text-[11px] text-[var(--ct-muted-fg)]">{option.filename}</span></span>
                <span className="block text-[var(--ct-muted-fg)]">{option.disabledReason ?? option.hint}</span>
              </span>
            </label>
          ))}
        </fieldset>
      )}
      {error && <p role="alert" className="text-[12px] text-[var(--ct-red)]">{error}</p>}
      <div className="flex justify-end gap-2">
        {!result && <Button type="button" variant="ghost" onClick={onClose}>Cancel</Button>}
        <Button type="submit" variant="primary" loading={busy}>{result ? "Done" : "Export"}</Button>
      </div>
    </FormDialog>
  );
}
