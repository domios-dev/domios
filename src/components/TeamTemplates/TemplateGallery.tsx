import { useId, useRef, useState, type RefObject } from "react";
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
import { PinnedFooter } from "./PinnedFooter";
import { useSelectedTemplateCard } from "./useSelectedTemplateCard";
import { templateCatalogKey, type TemplateCatalogStatus } from "./useTemplateCatalogRefusals";
import type { BlockedTemplate, TeamTemplateChoice } from "./useTeamTemplateChoice";

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
const isBlocked = (summary: TemplateSummary, blocked: BlockedTemplate | null) =>
  blocked !== null && blocked.name === summary.name && (blocked.version === null || blocked.version === summary.version);

function TemplateCard({ summary, selected, status, confirming, choice, footerRef, onSelect, onExport, onDelete, onCancelDelete, onConfirmDelete }: {
  summary: TemplateSummary;
  selected: boolean;
  status: TemplateCatalogStatus | undefined;
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
  const reasonId = useId();
  const ready = status?.kind === "ready";
  const blocked = status?.kind === "refused";
  const failure = status && status.kind !== "ready" ? status.reason : null;
  const reason = failure ? [failure.message, ...failure.reasons].join(" ") : undefined;
  const cardRef = useSelectedTemplateCard(selected && !confirming, reason);
  const source = templateSource(summary);
  const byline = cardByline(summary);
  const [placement, setPlacement] = useState<"bottom-end" | "top-end">("bottom-end");
  const menuItems = [
    { label: "Details", onClick: () => { onSelect(); choice.showSummaryDetails(summary); } },
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
    <li ref={cardRef} data-blocked={blocked || undefined} data-selected={selected} className="ct-card ct-card--choice template-gallery-card group flex min-w-0 flex-col">
      <div className="flex min-w-0 items-start gap-1">
        <button type="button" aria-pressed={selected} aria-label={reason ? `${title}: ${reason}` : undefined} aria-describedby={reason ? reasonId : undefined} title={reason} onClick={onSelect} onDoubleClick={() => { if (ready) choice.choose(summary); }}
          className="flex min-w-0 flex-1 items-center gap-3 rounded-[inherit] py-2.5 pl-3 text-left outline-none focus-visible:ring-1 focus-visible:ring-[var(--ct-accent)]">
          <TemplateIcon icon={summary.icon} />
          <span className="min-w-0 flex-1">
            <span className="flex items-baseline gap-2">
              <span className="truncate text-[13px] font-medium">{title}</span>
              {summary.version && <span className="font-mono text-[11px] text-[var(--ct-muted-fg)]">{summary.version}</span>}
            </span>
            <span className="block truncate text-[12px] text-[var(--ct-muted-fg)]" title={summary.description}>{summary.description}</span>
          </span>
        </button>
        <div className="flex shrink-0 items-center gap-1 self-start py-2.5 pr-2">
          <span className="grid h-7 w-[90px] shrink-0 items-center justify-items-end">
            {!ready ? <span className={`text-[11px] ${blocked ? "text-[var(--ct-red)]" : "text-[var(--ct-muted-fg)]"}`}
              title={reason} data-testid={blocked ? "template-card-blocked" : undefined}>
              {blocked ? "Can't start" : status ? "Unavailable" : "Checking…"}
            </span> : (
            <Button variant="ghost" className="text-xs opacity-0 transition-opacity focus-visible:opacity-100 group-hover:opacity-100 group-focus-within:opacity-100"
              aria-label={`Use ${title}`} onClick={() => { onSelect(); choice.choose(summary); }}>Use</Button>)}
          </span>
          {reason && <span id={reasonId} className="sr-only">{reason}</span>}
          {summary.provenance !== "builtin" && (
            <span className="inline-flex" onPointerDownCapture={(event) => placeMenu(event.currentTarget)}
              onKeyDownCapture={(event) => placeMenu(event.currentTarget)}>
              <DropdownMenu menuLabel={`${title} actions`} placement={placement} items={menuItems}
                trigger={<Button variant="icon" aria-label={`More actions for ${title}`} aria-haspopup="menu"><KebabGlyph /></Button>} />
            </span>
          )}
        </div>
      </div>
      <span className="flex min-w-0 items-center gap-1 overflow-hidden px-3 pb-2.5 text-[11px] text-[var(--ct-muted-fg)]">
        <span className="inline-flex min-w-0 max-w-full shrink-0 items-center gap-1">
          <TemplateTrust summary={summary} inline />{byline && <span aria-hidden="true" className="shrink-0">·</span>}
        </span>
        {byline && <span className="min-w-0 flex-1 truncate" title={byline} data-testid="template-byline">{byline}</span>}
      </span>
      {selected && failure && <div role="alert" data-testid="template-blocked-start" aria-label={`${title}: ${reason}`}
        className="border-t border-[var(--ct-border)] px-3 py-2 text-[12px]">
        <div className="font-medium">{title}</div>
        <div>{failure.message}</div>
        {failure.reasons.map((detail) => <div key={detail}>{detail}</div>)}
      </div>}
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
  const status = (summary: TemplateSummary): TemplateCatalogStatus | undefined => isBlocked(summary, choice.blocked)
    ? { kind: "refused", reason: choice.blocked!.refusal }
    : choice.catalogStatuses[templateCatalogKey(summary)];
  const targetStatus = target ? status(target) : undefined;
  const targetReady = targetStatus?.kind === "ready";
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
          <TemplateCard key={`${summary.name}@${summary.version}`} summary={summary} selected={same(summary, selected)} status={status(summary)} choice={choice} footerRef={footerRef}
            confirming={confirming !== null && same(summary, confirming)}
            onSelect={() => setSelected(summary)} onExport={onExport} onDelete={() => setConfirming(summary)} onCancelDelete={() => setConfirming(null)}
            onConfirmDelete={() => { choice.remove(summary); setConfirming(null); }} />
        ))}
        {catalog && entries.length === 0 && <li className="grid flex-1 place-items-center px-6 text-center text-[12px] text-[var(--ct-muted-fg)]" data-testid="template-gallery-empty">
          {query.trim() ? "No templates match." : EMPTY[filter]}
        </li>}
        {catalog?.error && <li className="text-[12px] text-[var(--ct-muted-fg)]">{catalog.error}</li>}
      </ul>
      {/* Pinned like the form's footer, so a refusal above the list never pushes it out of the dialog. */}
      <PinnedFooter sentinelClassName="-mt-[11px]" className="bg-[var(--ct-surface)]">
        <div ref={zoneRef} data-testid="template-drop-zone" data-drag-over={isDragOver || undefined} data-invalid={choice.notice ? true : undefined}
          className={`rounded-lg border border-dashed px-3 py-2.5 text-center text-[12px] text-[var(--ct-muted-fg)] ${choice.notice ? "border-[var(--ct-red)]" : isDragOver ? "border-[var(--ct-accent)]" : "border-[var(--ct-border)]"}`}>
          {choice.notice
            ? <span role="alert">{choice.notice}</span>
            : isDragOver ? "Release to preview" : <>Drop a <code>.team</code>, <code>.team.yaml</code> or folder here<OpenPackage choice={choice} /></>}
        </div>
        <div ref={footerRef} className="mt-2.5 flex justify-end">
          <Button variant="primary" className="text-xs" loading={choice.pending} disabled={!targetReady}
            onClick={() => { if (target && targetReady) choice.choose(target); }}>Use template</Button>
        </div>
      </PinnedFooter>
    </div>
  );
}
