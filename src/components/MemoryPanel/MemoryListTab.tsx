import { MemoryProvenance } from "./MemoryProvenance";
import { daemonFetch } from "../../lib/webApi";
import { memoryListControlsVisible } from "./memoryDetailModel";
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { Network, Search } from "lucide-react";
import { formatAge } from "../../lib/timeLabels";
import type { ApiConfig } from "../../lib/types";
import type { MemNode } from "./types";
import { colorForKind } from "./nodeKind";
import { authHeaders } from "./memoryApi";
import { MemoryItemEditor } from "./MemoryItemEditor";
import { Button } from "../shared/Button";
import { UI_INPUT } from "../shared/design";

interface Props {
  api: ApiConfig;
  nodes: Map<string, MemNode>;
  selectedId: string | null;
  onDetailOpenChange?: (open: boolean) => void;
  onSelectItem: (id: string | null) => void;
  onViewInGraph: (id: string) => void;
  viewToggle?: ReactNode;
  updateNode: (id: string, patch: Partial<MemNode>, markDirty?: boolean) => void;
  removeNode: (id: string) => void;
}

type SortKey = "recent" | "oldest" | "importance" | "title";

const KINDS = ["skill", "fact", "note", "learning", "rule", "preference", "conversation", "peer", "script", "daily", "summary"];

function relTime(iso: string): string {
  return formatAge(new Date(iso).getTime(), Date.now());
}

const PAGE_SIZE = 50;
const filterClass = "inline-flex h-[26px] shrink-0 cursor-pointer items-center gap-1.5 rounded-full !border px-2.5 text-xs focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-[var(--ct-accent)]";

function MemoryListTabImpl({ api, nodes, onViewInGraph, viewToggle, onSelectItem, onDetailOpenChange, removeNode }: Props) {
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState<SortKey>("recent");
  const [kindFilter, setKindFilter] = useState<Set<string>>(new Set());
  const [areaFilter, setAreaFilter] = useState<string | null>(null);
  const [visibleCount, setVisibleCount] = useState(PAGE_SIZE);
  const [editorItemId, setEditorItemId] = useState<string | null>(null);
  useEffect(() => {
    onDetailOpenChange?.(editorItemId !== null);
    return () => onDetailOpenChange?.(false);
  }, [editorItemId, onDetailOpenChange]);
  const searchRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  // Search via API (debounced, with AbortController to prevent stale results)
  const [searchResults, setSearchResults] = useState<string[] | null>(null);
  useEffect(() => {
    if (!search.trim()) { setSearchResults(null); return; }
    const ctrl = new AbortController();
    const timer = setTimeout(async () => {
      try {
        const params = new URLSearchParams({ q: search, limit: "50", supporter: "false" });
        const res = await daemonFetch(api, `/api/mem/search?${params}`, {
          headers: authHeaders(api),
          signal: ctrl.signal,
        });
        if (!res.ok) return;
        const data = await res.json();
        setSearchResults((data.hits || []).map((h: { item: { id: string } }) => h.item.id));
      } catch { /* ignore (includes AbortError) */ }
    }, 300);
    return () => { clearTimeout(timer); ctrl.abort(); };
  }, [search, api.host, api.port, api.token]);

  const areas = useMemo(() => {
    const s = new Set<string>();
    for (const n of nodes.values()) if (n.area) s.add(n.area);
    return [...s].sort();
  }, [nodes]);

  const filtered = useMemo(() => {
    let list = [...nodes.values()].filter(n => !n.tags?.includes("entity"));
    if (searchResults) {
      const ids = new Set(searchResults);
      list = list.filter(n => ids.has(n.id));
      // Preserve search result order (Map for O(1) index lookup)
      const orderMap = new Map(searchResults.map((id, i) => [id, i]));
      list.sort((a, b) => (orderMap.get(a.id) ?? 0) - (orderMap.get(b.id) ?? 0));
    } else {
      if (kindFilter.size > 0) list = list.filter(n => kindFilter.has(n.kind));
      if (areaFilter) list = list.filter(n => n.area === areaFilter);
      switch (sort) {
        case "recent": list.sort((a, b) => (b.updated_at || b.created_at || "").localeCompare(a.updated_at || a.created_at || "")); break;
        case "oldest": list.sort((a, b) => (a.created_at || "").localeCompare(b.created_at || "")); break;
        case "importance": list.sort((a, b) => (b.importance ?? 0) - (a.importance ?? 0)); break;
        case "title": list.sort((a, b) => a.title.localeCompare(b.title)); break;
      }
    }
    return list;
  }, [nodes, searchResults, kindFilter, areaFilter, sort]);

  const toggleKind = useCallback((k: string) => {
    setKindFilter(prev => {
      const next = new Set(prev);
      if (next.has(k)) next.delete(k); else next.add(k);
      return next;
    });
  }, []);

  useEffect(() => { setVisibleCount(PAGE_SIZE); }, [search, sort, kindFilter, areaFilter]);

  const visibleItems = useMemo(() => filtered.slice(0, visibleCount), [filtered, visibleCount]);
  const hasMore = visibleCount < filtered.length;

  const handleScroll = useCallback(() => {
    const el = listRef.current;
    if (!el || !hasMore) return;
    if (el.scrollTop + el.clientHeight >= el.scrollHeight - 100) {
      setVisibleCount(prev => Math.min(prev + PAGE_SIZE, filtered.length));
    }
  }, [hasMore, filtered.length]);

  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      if (e.key === "/") { e.preventDefault(); searchRef.current?.focus(); }
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, []);

  return (
    <div className="absolute inset-0 flex flex-col" data-testid="memory-list-tab">
      {memoryListControlsVisible(editorItemId !== null) && (
        <div className="grid shrink-0 grid-cols-[minmax(0,1fr)_104px_auto] items-center gap-x-2 gap-y-2.5 px-4 pb-2 pt-3 sm:grid-cols-[minmax(0,1fr)_128px_auto] sm:px-6 sm:pt-3.5">
          <div className="relative col-span-2 min-w-0 sm:col-span-1">
            <Search size={14} className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-[var(--ct-muted-fg)]" aria-hidden="true" />
            <input
              ref={searchRef}
              data-testid="memory-list-search"
              className={`${UI_INPUT.full} !h-8 !min-h-0 !rounded-lg !pl-8 !text-xs`}
              placeholder="Search memory... (/ to focus)"
              value={search}
              onChange={e => setSearch(e.target.value)}
              onKeyDown={e => { if (e.key === "Escape") { setSearch(""); (e.target as HTMLInputElement).blur(); } }}
            />
          </div>
          <select
            data-testid="memory-list-sort"
            className={`${UI_INPUT.select} col-start-2 row-start-3 !h-7 !min-h-0 !rounded-lg !text-xs sm:row-start-1 sm:!h-8`}
            value={sort}
            onChange={e => setSort(e.target.value as SortKey)}
          >
            <option value="recent">Recent</option>
            <option value="oldest">Oldest</option>
            <option value="importance">Importance</option>
            <option value="title">Title A-Z</option>
          </select>
          <div className="col-start-3 row-start-1 justify-self-end">{viewToggle}</div>
          <div className="col-span-full row-start-2 -my-0.5 flex min-w-0 items-center gap-1.5 overflow-x-auto py-0.5 pr-10 [mask-image:linear-gradient(to_right,black_calc(100%_-_40px),transparent)] [scrollbar-width:none]">
            {KINDS.map(k => {
              const active = kindFilter.has(k);
              const color = colorForKind(k);
              return (
                <button
                  key={k}
                  data-testid="memory-kind-filter"
                  data-memory-kind={k}
                  aria-pressed={active}
                  onClick={() => toggleKind(k)}
                  className={`${filterClass} ${active ? "!border-[var(--ct-accent)]/38 bg-[var(--ct-accent)]/14 text-[var(--ct-fg)]" : "!border-[var(--ct-border-subtle)] text-[var(--ct-muted-fg)] hover:bg-[var(--ct-bg-hover)]"}`}
                >
                  <span className="size-[7px] rounded-full" style={{ background: color }} />
                  {k}
                </button>
              );
            })}
          </div>
          <select
            data-testid="memory-area-filter"
            className={`${UI_INPUT.select} col-start-1 row-start-3 w-full max-w-[150px] !h-7 !min-h-0 !rounded-lg !text-xs`}
            value={areaFilter || ""}
            onChange={e => setAreaFilter(e.target.value || null)}
          >
            <option value="">All areas</option>
            {areas.map(a => <option key={a} value={a}>{a}</option>)}
          </select>
          <span className="col-start-3 row-start-3 justify-self-end whitespace-nowrap text-xs text-[var(--ct-muted-fg)] tabular-nums" data-testid="memory-list-count">{filtered.length} items</span>
        </div>
      )}
      <div ref={listRef} onScroll={handleScroll} className={`flex-1 overflow-y-auto px-1 sm:px-3 ${editorItemId ? "@min-[720px]/memory:mr-[440px]" : ""}`} data-testid="memory-list-results">
        {filtered.length === 0 && (
          <div className="text-xs text-[var(--ct-muted-fg)] text-center py-8" data-testid="memory-list-empty">
            {search ? `No results for "${search}"` : "No memories yet"}
          </div>
        )}
        {visibleItems.map(n => {
          const color = colorForKind(n.kind);
          return (
            <div
              key={n.id}
              className={`group rounded-[10px] ${editorItemId === n.id ? "bg-[light-dark(color-mix(in_srgb,var(--ct-bg-selected)_41%,var(--ct-bg)),color-mix(in_srgb,var(--ct-bg-selected)_50%,var(--ct-bg)))]" : "hover:bg-[var(--ct-bg-hover)]"}`}
              data-testid="memory-list-item"
              data-memory-id={n.id}
              data-memory-kind={n.kind}
            >
              <div className="grid cursor-pointer grid-cols-[8px_minmax(0,1fr)_auto] items-center gap-3 px-3 py-[9px]" onClick={() => setEditorItemId(n.id)}>
                <span className="mt-1.5 size-2 self-start rounded-full" style={{ background: color, outline: n.author?.kind === "neighbor" ? "1px solid var(--ct-accent)" : undefined, outlineOffset: 2 }} />
                <div className="min-w-0">
                  <div className="truncate text-[13.5px] font-semibold text-[var(--ct-fg)]">{n.title}</div>
                  <MemoryProvenance author={n.author} compact />
                  <div className="truncate text-xs text-[var(--ct-muted-fg)]">
                    <span className="font-[550]" style={{ color: `light-dark(color-mix(in srgb, ${color} 55%, var(--ct-fg)), ${color})` }}>{n.kind}</span>
                    {n.area && <> &middot; {n.area}</>}
                    {(n.recurrence_count ?? 0) > 0 && (
                      <>
                        {" · "}
                        <span className="text-[var(--ct-accent)]" title="Times this topic has recurred">
                          {n.recurrence_count}x seen
                        </span>
                      </>
                    )}
                    {(n.access_count ?? 0) >= 3 && (
                      <>
                        {" · "}
                        <span title="Access count">
                          ↗ {n.access_count}
                        </span>
                      </>
                    )}
                    {(n.tags?.length ?? 0) > 0 && (
                      <>
                        {" · "}
                        {(n.tags || []).filter(t => t !== "auto-extracted").slice(0, 3).join(", ")}
                      </>
                    )}
                  </div>
                </div>
                <div className="flex items-center gap-2">
                  <Button
                    variant="icon"
                    size="sm"
                    data-testid="memory-list-view-graph"
                    aria-label="View in graph"
                    title="View in graph"
                    className="!size-7 !min-h-0 !border !border-[var(--ct-border-subtle)] !px-0 pointer-fine:opacity-0 group-hover:opacity-100 group-focus-within:opacity-100"
                    onClick={(event) => {
                      event.stopPropagation();
                      onViewInGraph(n.id);
                    }}
                  >
                    <Network size={13} aria-hidden="true" />
                  </Button>
                  {n.updated_at && <span className="text-xs text-[var(--ct-muted-fg)] tabular-nums">{relTime(n.updated_at)}</span>}
                </div>
              </div>
            </div>
          );
        })}
        {hasMore && (
          <div className="text-[10px] text-[var(--ct-muted-fg)] text-center py-3" data-testid="memory-list-load-more-hint">
            {filtered.length - visibleCount} more — scroll to load
          </div>
        )}
      </div>
      {editorItemId && (
        <MemoryItemEditor
          itemId={editorItemId}
          api={api}
          onClose={() => setEditorItemId(null)}
          onViewInGraph={onViewInGraph}
          onDeleted={(id) => { removeNode(id); onSelectItem(null); }}
          onUpdated={() => { /* graph refetch will pick up changes */ }}
        />
      )}
    </div>
  );
}

export const MemoryListTab = memo(MemoryListTabImpl);
