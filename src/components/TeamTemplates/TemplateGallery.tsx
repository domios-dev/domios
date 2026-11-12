import { useRef, useState } from "react";
import { BackPill } from "../shared/BackPill";
import { Segmented } from "../shared/Segmented";
import { Button } from "../shared/Button";
import { DropdownMenu } from "../shared/DropdownMenu";
import { KebabGlyph } from "../shared/RowMenuButton";
import { nativeBridge } from "../../lib/nativeBridge";
import { useTauriFileDrop } from "../../hooks/terminal/useTauriFileDrop";
import { galleryEntries, templateSource, templateTitle, type GalleryFilter, type TemplateSummary } from "../../lib/teamTemplates/startModel";
import { TemplateIcon } from "./TemplateIcon";
import { TemplateTrust } from "./TemplateTrust";
import { cardByline } from "../../lib/teamTemplates/exchangeModel";
import type { TeamTemplateChoice } from "./useTeamTemplateChoice";

const FILTERS: { value: GalleryFilter; label: string }[] = [
  { value: "all", label: "All" }, { value: "builtin", label: "Built-in" }, { value: "mine", label: "Mine" }, { value: "friends", label: "From friends" },
];

const same = (a: TemplateSummary, b: TemplateSummary) => a.name === b.name && a.version === b.version;

function TemplateCard({ summary, selected, choice, onSelect, onExport, onDelete }: {
  summary: TemplateSummary;
  selected: boolean;
  choice: TeamTemplateChoice;
  onSelect: () => void;
  onExport: (summary: TemplateSummary, share?: boolean) => void;
  onDelete: () => void;
}) {
  const title = templateTitle(summary);
  const source = templateSource(summary);
  const byline = cardByline(summary, source.owned);
  return (
    <li className={`ct-card group flex min-w-0 items-start gap-1 ${selected ? "border-[color-mix(in_srgb,var(--ct-accent)_55%,transparent)] bg-[color-mix(in_srgb,var(--ct-accent)_7%,var(--ct-bg))]" : ""}`}>
      <button type="button" aria-pressed={selected} onClick={onSelect} onDoubleClick={() => choice.choose(summary)}
        className="flex min-w-0 flex-1 items-center gap-3 rounded-[inherit] py-2.5 pl-3 text-left outline-none focus-visible:ring-1 focus-visible:ring-[var(--ct-accent)]">
        <TemplateIcon icon={summary.icon} />
        <span className="min-w-0 flex-1">
          <span className="flex items-baseline gap-2">
            <span className="truncate text-[13px] font-medium">{title}</span>
            {summary.version && <span className="font-mono text-[11px] text-[var(--ct-muted-fg)]">{summary.version}</span>}
          </span>
          <span className="block truncate text-[12px] text-[var(--ct-muted-fg)]" title={summary.description}>{summary.description}</span>
          {byline && <span className="block truncate text-[11px] text-[var(--ct-muted-fg)]" data-testid="template-byline">{byline}</span>}
        </span>
      </button>
      <div className="flex shrink-0 items-center gap-1 self-start py-2.5 pr-2">
        <Button variant="ghost" className="text-xs opacity-0 transition-opacity focus-visible:opacity-100 group-hover:opacity-100 group-focus-within:opacity-100"
          aria-label={`Use ${title}`} onClick={() => choice.choose(summary)}>Use</Button>
        <TemplateTrust summary={summary} />
        {summary.provenance !== "builtin" && (
          <DropdownMenu menuLabel={`${title} actions`} placement="bottom-end" items={[
            { label: "Details", onClick: () => choice.showSummaryDetails(summary) },
            ...(source.owned ? [
              { label: "Export…", onClick: () => onExport(summary) },
              { label: "Share to Neighborhood…", onClick: () => onExport(summary, true) },
            ] : []),
            { label: "Delete", onClick: onDelete },
          ]} trigger={<Button variant="icon" aria-label={`More actions for ${title}`} aria-haspopup="menu"><KebabGlyph /></Button>} />
        )}
      </div>
    </li>
  );
}

export function TemplateGallery({ choice, onExport }: { choice: TeamTemplateChoice; onExport: (summary: TemplateSummary, share?: boolean) => void }) {
  const [filter, setFilter] = useState<GalleryFilter>("all");
  const [query, setQuery] = useState("");
  const current = choice.chosen?.form.template ?? choice.defaultSummary;
  const [selected, setSelected] = useState<TemplateSummary>(current);
  const [confirming, setConfirming] = useState<TemplateSummary | null>(null);
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
        {entries.map((summary) => (
          <TemplateCard key={`${summary.name}@${summary.version}`} summary={summary} selected={same(summary, selected)} choice={choice}
            onSelect={() => setSelected(summary)} onExport={onExport} onDelete={() => setConfirming(summary)} />
        ))}
        {catalog?.error && <li className="text-[12px] text-[var(--ct-muted-fg)]">{catalog.error}</li>}
        {confirming && (
          <li role="alertdialog" aria-label={`Delete ${templateTitle(confirming)}`} className="ct-card ct-card--static flex flex-wrap items-center gap-2 p-2.5 text-[12px]">
            <span className="min-w-0 flex-1">Delete {templateTitle(confirming)} {confirming.version} from My templates? Teams already started keep running.</span>
            <Button variant="ghost" className="text-xs" onClick={() => setConfirming(null)}>Cancel</Button>
            <Button variant="danger" className="text-xs" onClick={() => { choice.remove(confirming); setConfirming(null); }}>Delete</Button>
          </li>
        )}
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
