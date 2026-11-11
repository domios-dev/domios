import { useEffect, useId, useMemo, useState } from "react";
import { Eye } from "lucide-react";
import { FormDialog } from "../shared/FormDialog";
import { Button } from "../shared/Button";
import { DropdownSelect } from "../shared/DropdownMenu";
import { teamTemplatesTransport } from "../../lib/transport/orchestrationRuntime";
import type { TeamTemplatesTransport, TemplateDistillRequest, TemplateDistillResult } from "../../lib/transport/teamTemplates";
import { startErrorView, type TemplateSummary } from "../../lib/teamTemplates/startModel";
import { distillName, SAVE_VERSION_DEFAULT } from "../../lib/teamTemplates/exchangeModel";
import { TemplateBrings } from "./TemplateBrings";
import { ScrubReportCard, TemplateExportDialog } from "./TemplateExportDialog";

const LICENSES = ["", "MIT", "Apache-2.0", "BSD-3-Clause", "MPL-2.0", "GPL-3.0-only", "Unlicense"];
const FIELD = "w-full rounded-full border border-[var(--ct-border)] bg-[var(--ct-bg)] px-3 py-1.5 text-[12px] outline-none focus:border-[var(--ct-accent)]";

interface Identity { title: string; description: string; version: string; license: string }

/** Bakes a live Team into My templates: the server reviews it first (dry run), then saves the exact package it showed. */
export function SaveTemplateDialog({ teamId, teamName, onClose, transport = teamTemplatesTransport }: {
  teamId: string;
  teamName: string;
  onClose: () => void;
  transport?: () => TeamTemplatesTransport;
}) {
  const titleId = useId();
  const descriptionId = useId();
  const [identity, setIdentity] = useState<Identity>({ title: teamName, description: "", version: SAVE_VERSION_DEFAULT, license: "" });
  const [optOut, setOptOut] = useState<ReadonlySet<string>>(new Set());
  const [includes, setIncludes] = useState<TemplateDistillResult["includes"] | null>(null);
  const [review, setReview] = useState<TemplateDistillResult | null>(null);
  const [showYaml, setShowYaml] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [exporting, setExporting] = useState<TemplateSummary | null>(null);
  const [saved, setSaved] = useState<TemplateSummary | null>(null);

  const request = useMemo((): TemplateDistillRequest => ({
    team_id: teamId,
    name: distillName(identity.title),
    title: identity.title.trim() || null,
    description: identity.description.trim(),
    version: identity.version.trim(),
    license: identity.license || null,
    opt_out: [...optOut].sort(),
    format: "team",
  }), [identity, optOut, teamId]);

  useEffect(() => {
    const controller = new AbortController();
    transport().distill({ team_id: teamId, name: distillName(teamName), description: "", version: SAVE_VERSION_DEFAULT, format: "team", dry_run: true }, { signal: controller.signal })
      .then((result) => { setIncludes(result.includes); setReview(result); }, (failure: unknown) => {
        if (!controller.signal.aborted) setError(startErrorView(failure).message);
      });
    return () => controller.abort();
  }, [teamId, teamName, transport]);

  useEffect(() => {
    if (!showYaml) return;
    const controller = new AbortController();
    transport().distill({ ...request, dry_run: true }, { signal: controller.signal })
      .then(setReview, (failure: unknown) => { if (!controller.signal.aborted) setError(startErrorView(failure).message); });
    return () => controller.abort();
  }, [request, showYaml, transport]);

  const save = async (thenExport: boolean) => {
    setBusy(true);
    setError(null);
    try {
      const result = await transport().distill({ ...request, save: true });
      if (!result.check.valid || !result.saved) {
        setReview(result);
        setError(result.check.diagnostics[0]?.message ?? "The package didn't pass its check, so it wasn't saved.");
        return;
      }
      if (thenExport) setExporting(result.saved);
      else setSaved(result.saved);
    } catch (failure) {
      setError(startErrorView(failure).message);
    } finally {
      setBusy(false);
    }
  };

  if (exporting) return <TemplateExportDialog summary={exporting} onClose={onClose} transport={transport} />;
  const set = (key: keyof Identity) => (value: string) => setIdentity((current) => ({ ...current, [key]: value }));
  const ready = Boolean(request.title && request.description && request.version) && includes !== null;
  return (
    <FormDialog titleId={titleId} descriptionId={descriptionId} className="max-w-lg" onCancel={onClose}
      onSubmit={(event) => { event.preventDefault(); if (saved) onClose(); else if (ready) void save(false); }}>
      <h2 id={titleId} className="text-base font-semibold">Save as template</h2>
      <p id={descriptionId} className="text-sm text-[var(--ct-muted)]">
        {saved ? `Saved ${saved.title ?? saved.name} ${saved.version} to My templates.` : `From the team ${teamName}. Saved to My templates.`}
      </p>
      {!saved && <>
        <label className="flex flex-col gap-1 text-[11px] text-[var(--ct-muted-fg)]">Name
          <input className={FIELD} value={identity.title} onChange={(event) => set("title")(event.target.value)} required />
        </label>
        <label className="flex flex-col gap-1 text-[11px] text-[var(--ct-muted-fg)]">What it does
          <input className={FIELD} value={identity.description} onChange={(event) => set("description")(event.target.value)} required />
        </label>
        <div className="grid grid-cols-2 gap-3">
          <label className="flex flex-col gap-1 text-[11px] text-[var(--ct-muted-fg)]">Version
            <input className={`${FIELD} font-mono`} value={identity.version} onChange={(event) => set("version")(event.target.value)} required />
          </label>
          <div className="flex flex-col gap-1 text-[11px] text-[var(--ct-muted-fg)]">License
            <DropdownSelect aria-label="License" value={identity.license} onChange={set("license")}
              options={LICENSES.map((license) => ({ value: license, label: license || "None" }))} />
          </div>
        </div>
        <p className="text-[11px] text-[var(--ct-muted-fg)]">Keeps its managers and workers with their models and instructions, the asks and the inputs. Saved as <span className="font-mono">{request.name}</span>.</p>
        {includes === null && !error && <p className="text-[12px] text-[var(--ct-muted-fg)]" aria-busy="true">Reading the team…</p>}
        {includes && <TemplateBrings title="Includes" defaultOpen payload={includes} optOut={optOut}
          onToggle={(id, kept) => setOptOut((current) => { const next = new Set(current); if (kept) next.delete(id); else next.add(id); return next; })} />}
        {review && <div className="flex flex-col gap-1">
          <div className="text-[11px] text-[var(--ct-muted-fg)]">Removed before saving</div>
          <ScrubReportCard report={review.scrub} />
        </div>}
        <button type="button" aria-expanded={showYaml} onClick={() => setShowYaml(!showYaml)} className="flex items-center gap-1.5 self-start text-[12px] text-[var(--ct-accent)] hover:underline">
          <Eye size={12} aria-hidden="true" /> {showYaml ? "Hide team.yaml" : "Show team.yaml"}
        </button>
        {showYaml && review && <pre aria-label="team.yaml" className="max-h-64 overflow-auto rounded-md border border-[var(--ct-border)] bg-[var(--ct-bg)] p-2.5 font-mono text-[11px] leading-snug">{review.yaml}</pre>}
        {review && !review.check.valid && <ul className="text-[12px] text-[var(--ct-red)]">
          {review.check.diagnostics.map((diagnostic) => <li key={`${diagnostic.file}:${diagnostic.path}:${diagnostic.code}`}>{diagnostic.message}</li>)}
        </ul>}
      </>}
      {error && <p role="alert" className="text-[12px] text-[var(--ct-red)]">{error}</p>}
      <div className="flex justify-end gap-2">
        {saved ? <Button type="submit" variant="primary">Done</Button> : <>
          <Button type="button" variant="ghost" onClick={onClose}>Cancel</Button>
          <Button type="button" variant="ghost" disabled={!ready || busy} onClick={() => { void save(true); }}>Save and export…</Button>
          <Button type="submit" variant="primary" loading={busy} disabled={!ready}>Save template</Button>
        </>}
      </div>
    </FormDialog>
  );
}
