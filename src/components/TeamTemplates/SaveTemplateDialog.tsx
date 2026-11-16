import { useEffect, useId, useMemo, useState } from "react";
import { Eye } from "lucide-react";
import { FormDialog } from "../shared/FormDialog";
import { Button } from "../shared/Button";
import { DropdownSelect } from "../shared/DropdownMenu";
import { teamTemplatesTransport } from "../../lib/transport/orchestrationRuntime";
import type { TeamTemplatesTransport, TemplateDistillRequest, TemplateDistillResult } from "../../lib/transport/teamTemplates";
import { startErrorView, withKept, type TemplateSummary } from "../../lib/teamTemplates/startModel";
import { distillDescription, distillName, SAVE_VERSION_DEFAULT } from "../../lib/teamTemplates/exchangeModel";
import { TemplateBrings } from "./TemplateBrings";
import { ScrubReportCard, TemplateExportDialog } from "./TemplateExportDialog";

const LICENSES = ["", "MIT", "Apache-2.0", "BSD-3-Clause", "MPL-2.0", "GPL-3.0-only", "Unlicense"];
const FIELD = "w-full rounded-full border border-[var(--ct-border)] bg-[var(--ct-bg)] px-3 py-1.5 text-[12px] outline-none focus:border-[var(--ct-accent)]";

interface Identity { title: string; description: string; version: string; license: string }

/**
 * Bakes a live Team into My templates: the server reviews it first (dry run), then saves the exact package it showed.
 * A plain save closes the dialog and hands the saved template to `onSaved`, which confirms it with a toast.
 */
export function SaveTemplateDialog({ teamId, teamName, onClose, onSaved, transport = teamTemplatesTransport }: {
  teamId: string;
  teamName: string;
  onClose: () => void;
  onSaved: (saved: TemplateSummary) => void;
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

  // The first review fills Includes; later ones run only while it is missing or team.yaml is shown, so a fixed field recovers.
  const wantsReview = includes === null || showYaml;
  useEffect(() => {
    if (!wantsReview) return;
    const controller = new AbortController();
    // The server refuses an empty description even for a review; the dry run alone uses a default the owner never sees.
    transport().distill({ ...request, description: request.description || distillDescription(teamName), dry_run: true }, { signal: controller.signal })
      .then((result) => {
        setIncludes((current) => current ?? result.includes);
        setReview(result);
        setError(null);
      }, (failure: unknown) => { if (!controller.signal.aborted) setError(startErrorView(failure).message); });
    return () => controller.abort();
  }, [request, wantsReview, transport, teamName]);

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
      else {
        onSaved(result.saved);
        onClose();
      }
    } catch (failure) {
      setError(startErrorView(failure).message);
    } finally {
      setBusy(false);
    }
  };

  if (exporting) return <TemplateExportDialog summary={exporting} onClose={onClose} transport={transport} />;
  const set = (key: keyof Identity) => (value: string) => setIdentity((current) => ({ ...current, [key]: value }));
  const missing = !request.title ? "a name" : !request.description ? "what it does" : !request.version ? "a version" : null;
  const ready = missing === null && includes !== null;
  return (
    <FormDialog titleId={titleId} descriptionId={descriptionId} className="max-w-lg" onCancel={onClose}
      onSubmit={(event) => { event.preventDefault(); if (ready && !busy) void save(false); }}>
      <h2 id={titleId} className="text-base font-semibold">Save as template</h2>
      <p id={descriptionId} className="text-sm text-[var(--ct-muted)]">From the team {teamName}. Saved to My templates.</p>
        <label className="flex flex-col gap-1 text-[11px] text-[var(--ct-muted-fg)]">Name
          <input className={FIELD} value={identity.title} onChange={(event) => set("title")(event.target.value)} required />
        </label>
        <label className="flex flex-col gap-1 text-[11px] text-[var(--ct-muted-fg)]">What it does
          <input className={FIELD} value={identity.description} placeholder="One line on what this team does" onChange={(event) => set("description")(event.target.value)} required />
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
        <p className="text-[11px] text-[var(--ct-muted-fg)]">Keeps its managers and workers with their models and instructions, the asks and the inputs.</p>
        {includes === null && !error && <p className="text-[12px] text-[var(--ct-muted-fg)]" aria-busy="true">Reading the team…</p>}
        {includes && <TemplateBrings title="Includes" defaultOpen payload={includes} optOut={optOut}
          onToggle={(ids, kept) => setOptOut((current) => withKept(current, ids, kept))} />}
        {review && <div className="flex flex-col gap-1">
          <div className="text-[11px] text-[var(--ct-muted-fg)]">Removed before saving</div>
          <ScrubReportCard report={review.scrub} />
        </div>}
        <button type="button" aria-expanded={showYaml} onClick={() => setShowYaml(!showYaml)} className="flex items-center gap-1.5 self-start text-[12px] text-[var(--ct-accent)] hover:underline">
          <Eye size={12} aria-hidden="true" /> {showYaml ? "Hide team.yaml" : "Show team.yaml"}
        </button>
        {showYaml && review && !request.description && <p className="text-[11px] text-[var(--ct-muted-fg)]">Add what it does to see its team.yaml.</p>}
        {showYaml && review && request.description && <pre aria-label="team.yaml" className="max-h-64 overflow-auto rounded-md border border-[var(--ct-border)] bg-[var(--ct-bg)] p-2.5 font-mono text-[11px] leading-snug">{review.yaml}</pre>}
        {review && !review.check.valid && <ul className="text-[12px] text-[var(--ct-red)]">
          {review.check.diagnostics.map((diagnostic) => <li key={`${diagnostic.file}:${diagnostic.path}:${diagnostic.code}`}>{diagnostic.message}</li>)}
        </ul>}
      {error && <p role="alert" className="text-[12px] text-[var(--ct-red)]">{error}</p>}
      {!error && missing && <p role="status" className="text-[12px] text-[var(--ct-muted-fg)]">Add {missing} to save it.</p>}
      <div className="flex justify-end gap-2">
        <Button type="button" variant="ghost" className="text-xs" onClick={onClose}>Cancel</Button>
        <Button type="button" variant="ghost" className="text-xs" disabled={!ready || busy} onClick={() => { void save(true); }}>Save and export…</Button>
        <Button type="submit" variant="primary" className="text-xs" loading={busy} disabled={!ready}>Save template</Button>
      </div>
    </FormDialog>
  );
}
