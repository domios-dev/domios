import { useRef, useState } from "react";
import { BackPill } from "../shared/BackPill";
import { Segmented } from "../shared/Segmented";
import { Button } from "../shared/Button";
import { DropdownMenu } from "../shared/DropdownMenu";
import { KebabGlyph } from "../shared/RowMenuButton";
import { nativeBridge } from "../../lib/nativeBridge";
import { useTauriFileDrop } from "../../hooks/terminal/useTauriFileDrop";
import { galleryEntries, sourceLabel, templateTitle, type GalleryFilter, type TemplateSummary } from "../../lib/teamTemplates/startModel";
import { TemplateIcon } from "./TemplateIcon";
import type { TeamTemplateChoice } from "./useTeamTemplateChoice";

const FILTERS: { value: GalleryFilter; label: string }[] = [
  { value: "all", label: "All" }, { value: "builtin", label: "Built-in" }, { value: "mine", label: "Mine" }, { value: "friends", label: "From friends" },
];

const same = (a: TemplateSummary, b: TemplateSummary) => a.name === b.name && a.version === b.version;

function SourceTag({ summary }: { summary: TemplateSummary }) {
  const shared = summary.provenance === "imported";
  return (
    <span className={`shrink-0 text-[11px] ${shared ? "" : "ct-small-pill"} text-[var(--ct-muted-fg)]`}>
      {shared && <span aria-hidden="true" className="mr-1 inline-block h-1.5 w-1.5 rounded-full bg-[var(--ct-yellow,#d97706)]" />}
      {sourceLabel(summary.provenance)}
    </span>
  );
}

export function TemplateGallery({ choice, onExport }: { choice: TeamTemplateChoice; onExport: (summary: TemplateSummary) => void }) {
  const [filter, setFilter] = useState<GalleryFilter>("all");
  const [query, setQuery] = useState("");
  const current = choice.chosen?.form.template ?? choice.defaultSummary;
  const [selected, setSelected] = useState<TemplateSummary>(current);
  const zoneRef = useRef<HTMLDivElement>(null);
  const { isDragOver } = useTauriFileDrop({ containerRef: zoneRef, onPaths: (paths) => paths[0] && choice.importPath(paths[0]), onError: choice.setNotice });
  const catalog = choice.catalog;
  const entries = galleryEntries(catalog?.templates ?? [], catalog?.recall ?? null, choice.defaultSummary, filter, query);
  const openFile = async () => {
    const picked = await nativeBridge.dialogs?.open({ multiple: false, directory: false, filters: [{ name: "Team package", extensions: ["team", "yaml"] }] });
    if (typeof picked === "string") choice.importPath(picked);
  };
  return (
    <div className="flex flex-col gap-2.5" data-testid="template-gallery">
      <div className="flex items-center gap-2">
        <BackPill label="the Team form" onBack={choice.backToForm} />
        <input type="search" aria-label="Search templates" placeholder="Search templates" value={query} onChange={(event) => setQuery(event.target.value)}
          className="min-w-0 flex-1 rounded-full border border-[var(--ct-border)] bg-[var(--ct-bg)] px-3 py-1.5 text-[12px] outline-none" />
      </div>
      <Segmented aria-label="Template source" value={filter} options={FILTERS} onChange={setFilter} />
      <ul className={`flex max-h-[340px] flex-col gap-1.5 overflow-y-auto ${isDragOver ? "opacity-40" : ""}`} aria-label="Templates" aria-busy={!catalog}>
        {entries.map((summary) => {
          const isSelected = same(summary, selected);
          return (
            <li key={`${summary.name}@${summary.version}`} className="flex items-start gap-1">
              <button type="button" aria-pressed={isSelected} onClick={() => setSelected(summary)} onDoubleClick={() => choice.choose(summary)}
                className={`ct-card flex min-w-0 flex-1 items-start gap-3 p-3 text-left ${isSelected ? "border-[var(--ct-accent)]" : ""}`}>
                <TemplateIcon icon={summary.icon} />
                <span className="min-w-0 flex-1">
                  <span className="flex items-baseline gap-2">
                    <span className="truncate text-[13px] font-medium">{templateTitle(summary)}</span>
                    {summary.version && <span className="font-mono text-[11px] text-[var(--ct-muted-fg)]">{summary.version}</span>}
                  </span>
                  <span className="block truncate text-[12px] text-[var(--ct-muted-fg)]">{summary.description}</span>
                </span>
                <SourceTag summary={summary} />
              </button>
              {summary.provenance === "saved" && (
                <DropdownMenu menuLabel={`${templateTitle(summary)} actions`} items={[
                  { label: "Details", onClick: () => choice.showSummaryDetails(summary) },
                  { label: "Export…", onClick: () => onExport(summary) },
                ]} trigger={<Button variant="icon" aria-label={`More actions for ${templateTitle(summary)}`} aria-haspopup="menu"><KebabGlyph /></Button>} />
              )}
            </li>
          );
        })}
        {catalog?.error && <li className="text-[12px] text-[var(--ct-muted-fg)]">{catalog.error}</li>}
      </ul>
      <div ref={zoneRef} data-testid="template-drop-zone" data-drag-over={isDragOver || undefined} data-invalid={choice.notice ? true : undefined}
        className={`rounded-lg border border-dashed px-3 py-2.5 text-center text-[12px] text-[var(--ct-muted-fg)] ${choice.notice ? "border-[var(--ct-red)]" : isDragOver ? "border-[var(--ct-accent)]" : "border-[var(--ct-border)]"}`}>
        {choice.notice
          ? <span role="alert">{choice.notice}</span>
          : isDragOver ? "Release to preview" : <>Drop a <code>.team</code>, <code>.team.yaml</code> or folder here{nativeBridge.dialogs && <>, or <button type="button" className="text-[var(--ct-accent)] hover:underline" onClick={() => { void openFile(); }}>open one</button></>}</>}
      </div>
      <div className="flex justify-end">
        <Button variant="primary" className="text-xs" loading={choice.pending} onClick={() => choice.choose(selected)}>Use template</Button>
      </div>
    </div>
  );
}
