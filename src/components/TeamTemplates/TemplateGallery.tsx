import { useRef, useState, type RefObject } from "react";
import { BackPill } from "../shared/BackPill";
import { Segmented } from "../shared/Segmented";
import { Button } from "../shared/Button";
import { DropdownMenu } from "../shared/DropdownMenu";
import { KebabGlyph } from "../shared/RowMenuButton";
import { nativeBridge } from "../../lib/nativeBridge";
import { useTauriFileDrop } from "../../hooks/terminal/useTauriFileDrop";
import { galleryEntries, templateSource, templateTitle, type GalleryFilter, type TemplateSummary } from "../../lib/teamTemplates/startModel";
import { folderEntryPath, storedZip } from "../../lib/teamTemplates/storedZip";
import { TemplateIcon } from "./TemplateIcon";
import { TemplateTrust } from "./TemplateTrust";
import { cardByline } from "../../lib/teamTemplates/exchangeModel";
import type { TeamTemplateChoice } from "./useTeamTemplateChoice";

const FILTERS: { value: GalleryFilter; label: string }[] = [
  { value: "all", label: "All" }, { value: "builtin", label: "Built-in" }, { value: "mine", label: "Mine" }, { value: "friends", label: "From friends" },
];

const EMPTY: Record<GalleryFilter, string> = {
  all: "No templates match.",
  builtin: "No built-in template matches.",
  mine: "Templates you save or import appear here.",
  friends: "Teams shared with you in Neighborhood appear here.",
};

/** A row menu item is about 30 px tall; the menu flips above its kebab when the footer would sit under it. */
const MENU_ROW = 30;
const MENU_CHROME = 12;

const same = (a: TemplateSummary, b: TemplateSummary) => a.name === b.name && a.version === b.version;

function TemplateCard({ summary, selected, confirming, choice, footerRef, onSelect, onExport, onDelete, onCancelDelete, onConfirmDelete }: {
  summary: TemplateSummary;
  selected: boolean;
  confirming: boolean;
  choice: TeamTemplateChoice;
  footerRef: RefObject<HTMLElement | null>;
  onSelect: () => void;
  onExport: (summary: TemplateSummary, share?: boolean) => void;
  onDelete: () => void;
  onCancelDelete: () => void;
  onConfirmDelete: () => void;
}) {
  const title = templateTitle(summary);
  const source = templateSource(summary);
  const byline = cardByline(summary);
  const [placement, setPlacement] = useState<"bottom-end" | "top-end">("bottom-end");
  const menuItems = [
    { label: "Details", onClick: () => choice.showSummaryDetails(summary) },
    ...(source.owned ? [
      { label: "Export…", onClick: () => onExport(summary) },
      { label: "Share to Neighborhood…", onClick: () => onExport(summary, true) },
    ] : []),
    { label: "Delete", onClick: onDelete },
  ];
  // Decided before the menu opens, so it never covers the footer's primary button.
  const placeMenu = (trigger: HTMLElement) => {
    const below = (footerRef.current?.getBoundingClientRect().top ?? window.innerHeight) - trigger.getBoundingClientRect().bottom;
    setPlacement(below < menuItems.length * MENU_ROW + MENU_CHROME ? "top-end" : "bottom-end");
  };
  if (confirming) {
    return (
      <li role="group" aria-label={`Delete ${title}`} className="ct-card ct-card--static flex flex-wrap items-center gap-2 px-3 py-2.5 text-[12px]" data-testid="template-delete-confirm">
        <span className="min-w-0 flex-1">
          <span className="block font-medium">Delete {title}?</span>
          <span className="block text-[11px] text-[var(--ct-muted-fg)]">Teams already started keep running.</span>
        </span>
        <Button variant="ghost" className="text-xs" autoFocus onClick={onCancelDelete}>Cancel</Button>
        <Button variant="danger" className="text-xs" onClick={onConfirmDelete}>Delete</Button>
      </li>
    );
  }
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
          {/* Below 480 px the source joins this meta line instead of crowding the actions. */}
          <span className="flex min-w-0 items-center gap-1 text-[11px] text-[var(--ct-muted-fg)]">
            <span className="inline-flex shrink-0 items-center gap-1 min-[480px]:hidden">
              <TemplateTrust summary={summary} inline />{byline && <span aria-hidden="true">·</span>}
            </span>
            {byline && <span className="truncate" data-testid="template-byline">{byline}</span>}
          </span>
        </span>
      </button>
      <div className="flex shrink-0 items-center gap-1 self-start py-2.5 pr-2">
        {/* Use and the status share one slot: hover or focus swaps them, so the description keeps its width. */}
        <span className="grid items-center justify-items-end">
          <span className="col-start-1 row-start-1 hidden transition-opacity group-hover:opacity-0 group-focus-within:opacity-0 min-[480px]:inline-flex">
            <TemplateTrust summary={summary} />
          </span>
          <Button variant="ghost" className="col-start-1 row-start-1 text-xs opacity-0 transition-opacity focus-visible:opacity-100 group-hover:opacity-100 group-focus-within:opacity-100"
            aria-label={`Use ${title}`} onClick={() => choice.choose(summary)}>Use</Button>
        </span>
        {summary.provenance !== "builtin" && (
          <span className="inline-flex" onPointerDownCapture={(event) => placeMenu(event.currentTarget)}
            onKeyDownCapture={(event) => placeMenu(event.currentTarget)}>
            <DropdownMenu menuLabel={`${title} actions`} placement={placement} items={menuItems}
              trigger={<Button variant="icon" aria-label={`More actions for ${title}`} aria-haspopup="menu"><KebabGlyph /></Button>} />
          </span>
        )}
      </div>
    </li>
  );
}

/** "or open one": the native dialog on desktop; on the web a file input and a folder input feeding the same import. */
function OpenPackage({ choice }: { choice: TeamTemplateChoice }) {
  const fileRef = useRef<HTMLInputElement>(null);
  const folderRef = useRef<HTMLInputElement>(null);
  const link = "text-[var(--ct-accent)] hover:underline";
  if (nativeBridge.dialogs) {
    const openFile = async () => {
      const picked = await nativeBridge.dialogs?.open({ multiple: false, directory: false, filters: [{ name: "Team package", extensions: ["team", "yaml", "yml"] }] });
      if (typeof picked === "string") choice.importPath(picked);
    };
    return <>, or <button type="button" className={link} onClick={() => { void openFile(); }}>open one</button></>;
  }
  const pickFolder = async (files: readonly File[]) => {
    if (files.length === 0) return;
    const folder = files[0].webkitRelativePath.split("/")[0] || "package";
    const entries = await Promise.all(files.map(async (file) => ({
      path: folderEntryPath(file.webkitRelativePath || file.name),
      data: new Uint8Array(await file.arrayBuffer()),
    })));
    const zip = storedZip(entries);
    choice.importUpload(new Blob([zip.buffer as ArrayBuffer], { type: "application/zip" }), `${folder}.team`);
  };
  return <>
    , or <button type="button" className={link} onClick={() => fileRef.current?.click()}>open one</button>
    {" "}or a <button type="button" className={link} onClick={() => folderRef.current?.click()}>folder</button>
    <input ref={fileRef} type="file" accept=".team,.yaml,.yml" hidden aria-label="Open a team package" data-testid="template-open-file"
      onChange={(event) => {
        const file = event.target.files?.[0];
        event.target.value = "";
        if (file) choice.importUpload(file, file.name);
      }} />
    <input ref={folderRef} type="file" hidden aria-label="Open a team package folder" data-testid="template-open-folder"
      {...{ webkitdirectory: "", directory: "" }}
      onChange={(event) => {
        const files = Array.from(event.target.files ?? []);
        event.target.value = "";
        void pickFolder(files);
      }} />
  </>;
}

export function TemplateGallery({ choice, onExport }: { choice: TeamTemplateChoice; onExport: (summary: TemplateSummary, share?: boolean) => void }) {
  const [filter, setFilter] = useState<GalleryFilter>("all");
  const [query, setQuery] = useState("");
  const current = choice.chosen?.form.template ?? choice.defaultSummary;
  const [selected, setSelected] = useState<TemplateSummary>(current);
  const [confirming, setConfirming] = useState<TemplateSummary | null>(null);
  const zoneRef = useRef<HTMLDivElement>(null);
  const footerRef = useRef<HTMLDivElement>(null);
  const { isDragOver } = useTauriFileDrop({ containerRef: zoneRef, onPaths: (paths) => paths[0] && choice.importPath(paths[0]), onError: choice.setNotice });
  const catalog = choice.catalog;
  const entries = galleryEntries(catalog?.templates ?? [], catalog?.recall ?? null, choice.defaultSummary, filter, query);
  const target = entries.length > 0 ? selected : null;
  return (
    <div className="flex flex-col gap-2.5" data-testid="template-gallery">
      <div className="flex items-center gap-2">
        <BackPill label="the Team form" onBack={choice.backToForm} />
        <input type="search" aria-label="Search templates" placeholder="Search templates" value={query} onChange={(event) => setQuery(event.target.value)}
          className="min-w-0 flex-1 rounded-full border border-[var(--ct-border)] bg-[var(--ct-bg)] px-3 py-1.5 text-[12px] outline-none" />
      </div>
      <Segmented aria-label="Template source" value={filter} options={FILTERS} onChange={setFilter} />
      {/* A fixed height, so the dialog keeps its size while the filter changes. */}
      <ul className={`flex h-[340px] flex-col gap-1.5 overflow-y-auto ${isDragOver ? "opacity-40" : ""}`} aria-label="Templates" aria-busy={!catalog}>
        {entries.map((summary) => (
          <TemplateCard key={`${summary.name}@${summary.version}`} summary={summary} selected={same(summary, selected)} choice={choice} footerRef={footerRef}
            confirming={confirming !== null && same(summary, confirming)}
            onSelect={() => setSelected(summary)} onExport={onExport} onDelete={() => setConfirming(summary)} onCancelDelete={() => setConfirming(null)}
            onConfirmDelete={() => { choice.remove(summary); setConfirming(null); }} />
        ))}
        {catalog && entries.length === 0 && <li className="grid flex-1 place-items-center px-6 text-center text-[12px] text-[var(--ct-muted-fg)]" data-testid="template-gallery-empty">
          {query.trim() ? "No templates match." : EMPTY[filter]}
        </li>}
        {catalog?.error && <li className="text-[12px] text-[var(--ct-muted-fg)]">{catalog.error}</li>}
      </ul>
      <div ref={zoneRef} data-testid="template-drop-zone" data-drag-over={isDragOver || undefined} data-invalid={choice.notice ? true : undefined}
        className={`rounded-lg border border-dashed px-3 py-2.5 text-center text-[12px] text-[var(--ct-muted-fg)] ${choice.notice ? "border-[var(--ct-red)]" : isDragOver ? "border-[var(--ct-accent)]" : "border-[var(--ct-border)]"}`}>
        {choice.notice
          ? <span role="alert">{choice.notice}</span>
          : isDragOver ? "Release to preview" : <>Drop a <code>.team</code>, <code>.team.yaml</code> or folder here<OpenPackage choice={choice} /></>}
      </div>
      <div ref={footerRef} className="flex justify-end">
        <Button variant="primary" className="text-xs" loading={choice.pending} disabled={target === null}
          onClick={() => { if (target) choice.choose(target); }}>Use template</Button>
      </div>
    </div>
  );
}
