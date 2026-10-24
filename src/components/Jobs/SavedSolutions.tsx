import { useEffect, useMemo, useState } from "react";
import { BookOpen, Eye, Plus, Search, Trash2, X } from "lucide-react";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { Button } from "../shared/Button";
import { DropdownMenu } from "../shared/DropdownMenu";
import { JobRow } from "./JobRow";
import { useBlueprintLibraryRevision } from "./useBlueprintLibraryRevision";
import { blueprintSolutionsApi, type BlueprintSolutionsApi } from "./blueprintSolutions";
import { catalogEntry, catalogKinds, catalogTags, filterCatalog, NO_CATALOG_FILTER, type CatalogEntry, type CatalogFilter, type SolutionKind } from "./solutionCatalog";

const KIND_LABEL: Record<SolutionKind, string> = { pipeline: "Pipeline", watch: "Watch", call: "Callable" };

function lastUsed(seconds: number): string {
  return seconds ? new Date(seconds * 1000).toLocaleString() : "never";
}

export function solutionMeta(entry: CatalogEntry): string {
  const { solution } = entry;
  return [entry.kind ? KIND_LABEL[entry.kind] : null, `${solution.uses} uses`, `${solution.successes} successes`, `last used ${lastUsed(solution.last_used)}`, ...entry.tags].filter(Boolean).join(" · ");
}

function Capsule({ kind, value, onRemove }: { kind: string; value: string; onRemove: () => void }) {
  return <Button variant="capsule" aria-label={`Remove filter: ${kind} ${value}`} title={`Remove filter: ${kind} ${value}`} onClick={onRemove}>
    <span className="shrink-0 text-[var(--ct-muted-fg)]">{kind}</span>
    <span className="min-w-0 truncate font-medium">{value}</span>
    <X size={12} className="shrink-0 text-[var(--ct-muted-fg)]" aria-hidden="true" />
  </Button>;
}

/** Saved solutions as a catalogue: text filter while typing, semantic ranking on Enter, capsules for workspace, kind and tag. */
export function SavedSolutions({ sources, revision, api = blueprintSolutionsApi, workspaceName = (id) => id, onOpen }: {
  sources: readonly string[]; revision: number; api?: BlueprintSolutionsApi; workspaceName?: (id: string) => string; onOpen?: (entry: CatalogEntry) => void;
}) {
  const libraryRevision = useBlueprintLibraryRevision(sources);
  const [entries, setEntries] = useState<CatalogEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [filter, setFilter] = useState<CatalogFilter>(NO_CATALOG_FILTER);
  const [ranking, setRanking] = useState<{ query: string; ids: readonly string[] } | null>(null);
  const [searching, setSearching] = useState(false);
  useEffect(() => {
    let active = true;
    setEntries([]); setError(null);
    void Promise.all(sources.map(async (workspaceId) => (await api.list(workspaceId)).map((entry) => ({ ...entry, workspaceId })))).then(async (results) => {
      const solutions = results.flat();
      const stored = await Promise.all(solutions.map((solution) => api.document(solution.id).catch((failure: unknown) => failure instanceof Error ? failure : new Error(String(failure)))));
      if (active) setEntries(solutions.map((solution, index) => catalogEntry(solution, stored[index])));
    }, (failure: unknown) => { if (active) setError(String(failure)); });
    return () => { active = false; };
  }, [sources, revision, libraryRevision, api]);
  const shown = useMemo(() => filterCatalog(entries, filter, ranking?.query === filter.query ? ranking.ids : null), [entries, filter, ranking]);
  const kinds = catalogKinds(entries), tags = catalogTags(entries);
  const workspaces = [...new Set(entries.map((entry) => entry.solution.workspaceId))];
  const set = (next: Partial<CatalogFilter>) => setFilter((current) => ({ ...current, ...next }));
  const rank = async () => {
    const query = filter.query.trim();
    if (!query) { setRanking(null); return; }
    setSearching(true); setError(null);
    try {
      const scopes = filter.workspaceId ? [filter.workspaceId] : workspaces;
      setRanking({ query: filter.query, ids: (await Promise.all(scopes.map((workspaceId) => api.search(workspaceId, query)))).flat() });
    } catch (failure) { setError(String(failure)); } finally { setSearching(false); }
  };
  const remove = async (entry: CatalogEntry) => {
    setBusy(entry.solution.id); setError(null);
    try { await api.archive(entry.solution.workspaceId, entry.solution.id); setEntries((current) => current.filter((item) => item.solution.id !== entry.solution.id)); }
    catch (failure) { setError(String(failure)); }
    finally { setBusy(null); }
  };
  const groups = filter.workspaceId === null && workspaces.length > 1
    ? workspaces.map((workspaceId) => ({ workspaceId, entries: shown.filter((entry) => entry.solution.workspaceId === workspaceId) })).filter((group) => group.entries.length)
    : [{ workspaceId: null, entries: shown }];
  return <section aria-label="Saved solutions" className="mt-6 flex flex-col gap-3">
    <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
      <h3 className="ct-detail__heading m-0">Saved solutions</h3>
      {entries.length > 0 && <span className="text-xs text-[var(--ct-muted-fg)]">{shown.length === entries.length ? entries.length : `${shown.length} of ${entries.length}`}</span>}
    </div>
    {entries.length > 0 && <>
      <form role="search" className="ct-solution-search" onSubmit={(event) => { event.preventDefault(); void rank(); }}>
        <Search size={14} aria-hidden="true" />
        <input type="search" aria-label="Search saved solutions" placeholder="Describe what you need; Enter ranks by meaning" value={filter.query}
          onChange={(event) => set({ query: event.target.value })} />
        {searching && <span role="status" className="text-xs text-[var(--ct-muted-fg)]">Ranking…</span>}
      </form>
      <div role="group" aria-label="Saved solution filters" className="flex min-w-0 flex-wrap items-center gap-1.5">
        {filter.workspaceId && <Capsule kind="Workspace" value={workspaceName(filter.workspaceId)} onRemove={() => set({ workspaceId: null })} />}
        {filter.kind && <Capsule kind="Kind" value={KIND_LABEL[filter.kind]} onRemove={() => set({ kind: null })} />}
        {filter.tag && <Capsule kind="Tag" value={filter.tag} onRemove={() => set({ tag: null })} />}
        {!filter.workspaceId && workspaces.length > 1 && <DropdownMenu menuLabel="Filter by workspace" items={workspaces.map((id) => ({ label: workspaceName(id), onClick: () => set({ workspaceId: id }) }))}
          trigger={<Button variant="capsuleAdd" aria-label="Filter by workspace" aria-haspopup="menu"><Plus size={12} aria-hidden="true" />Workspace</Button>} />}
        {!filter.kind && kinds.length > 1 && <DropdownMenu menuLabel="Filter by kind" items={kinds.map((kind) => ({ label: KIND_LABEL[kind], onClick: () => set({ kind }) }))}
          trigger={<Button variant="capsuleAdd" aria-label="Filter by kind" aria-haspopup="menu"><Plus size={12} aria-hidden="true" />Kind</Button>} />}
        {!filter.tag && tags.length > 0 && <DropdownMenu menuLabel="Filter by tag" items={tags.map((tag) => ({ label: tag, onClick: () => set({ tag }) }))}
          trigger={<Button variant="capsuleAdd" aria-label="Filter by tag" aria-haspopup="menu"><Plus size={12} aria-hidden="true" />Tag</Button>} />}
      </div>
    </>}
    {error && <InlineErrorNotice error={error} operation="reading saved solutions" blocking />}
    {groups.map((group) => <div key={group.workspaceId ?? "all"} className="flex flex-col gap-2">
      {group.workspaceId && <h4 className="m-0 text-xs font-semibold text-[var(--ct-muted-fg)]">{workspaceName(group.workspaceId)}</h4>}
      <ul aria-label={group.workspaceId ? `Saved solutions in ${workspaceName(group.workspaceId)}` : "Saved solution list"} className="flex flex-col gap-2">{group.entries.map((entry) => <JobRow key={entry.solution.id} jobKey={`solution:${entry.solution.id}`}
        icon={<BookOpen size={15} aria-hidden="true" />} name={entry.solution.when_to_use} meta={entry.error ? `Unavailable: ${entry.error}` : solutionMeta(entry)} type="Saved solution" tone="blueprint" state={entry.kind ? KIND_LABEL[entry.kind] : "Saved"}
        actionItems={[
          ...(onOpen && entry.document ? [{ label: "Open", icon: <Eye size={12} aria-hidden="true" />, onClick: () => onOpen(entry) }] : []),
          { label: "Delete", icon: <Trash2 size={12} aria-hidden="true" />, danger: true, disabled: busy !== null, onClick: () => void remove(entry) },
        ]}
        onOpen={() => { if (onOpen && entry.document) onOpen(entry); }} onTriggerRef={() => {}} error={null} />)}</ul>
    </div>)}
    {entries.length === 0 && !error && <p className="ct-job-note">No saved solutions.</p>}
    {entries.length > 0 && shown.length === 0 && <p role="status" className="ct-job-note">No saved solution matches these filters.</p>}
  </section>;
}
