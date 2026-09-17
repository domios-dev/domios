import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ApiConfig } from "../../lib/types";
import type { MemNode } from "./types";
import { colorForKind } from "./nodeKind";
import { baseUrl, authHeaders } from "./memoryApi";
import { MemoryItemEditor } from "./MemoryItemEditor";

interface Props {
  api: ApiConfig;
  nodes: Map<string, MemNode>;
  selectedId: string | null;
  onSelectItem: (id: string | null) => void;
  onViewInGraph: (id: string) => void;
  updateNode: (id: string, patch: Partial<MemNode>, markDirty?: boolean) => void;
  removeNode: (id: string) => void;
}

type SortKey = "recent" | "oldest" | "importance" | "title";

const KINDS = ["skill", "fact", "note", "learning", "rule", "preference", "conversation", "peer", "script", "daily", "summary"];

function relTime(iso: string): string {
  const diff = Date.now() - new Date(iso).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return "now";
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  const days = Math.floor(hrs / 24);
  if (days < 30) return `${days}d ago`;
  return `${Math.floor(days / 30)}mo ago`;
}

const PAGE_SIZE = 50;

function MemoryListTabImpl({ api, nodes, onViewInGraph, onSelectItem, removeNode }: Props) {
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState<SortKey>("recent");
  const [kindFilter, setKindFilter] = useState<Set<string>>(new Set());
  const [areaFilter, setAreaFilter] = useState<string | null>(null);
  const [visibleCount, setVisibleCount] = useState(PAGE_SIZE);
  const [editorItemId, setEditorItemId] = useState<string | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  // Search via API (debounced, with AbortController to prevent stale results)
  const [searchResults, setSearchResults] = useState<string[] | null>(null);
  useEffect(() => {
    if (!search.trim()) { setSearchResults(null); return; }
    const ctrl = new AbortController();
    const timer = setTimeout(async () => {
      try {
        const params = new URLSearchParams({ q: search, limit: "50" });
        const res = await fetch(`${baseUrl(api)}/api/mem/search?${params}`, {
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
      {/* Toolbar */}
      <div className="flex items-center gap-2 px-3 py-2 border-b border-[var(--ct-border)]">
        <div className="relative flex-1">
          <svg className="absolute left-2 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-[var(--ct-muted-fg)]" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="6.5" cy="6.5" r="5" /><line x1="10" y1="10" x2="15" y2="15" /></svg>
          <input
            ref={searchRef}
            data-testid="memory-list-search"
            className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded pl-7 pr-2.5 py-1.5 text-xs text-[var(--ct-fg)] placeholder:text-[var(--ct-muted-fg)] focus:outline-none focus:border-[var(--ct-accent)] font-[inherit]"
            placeholder="Search memory... (/ to focus)"
            value={search}
            onChange={e => setSearch(e.target.value)}
            onKeyDown={e => { if (e.key === "Escape") { setSearch(""); (e.target as HTMLInputElement).blur(); } }}
          />
        </div>
        <select
          data-testid="memory-list-sort"
          className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-2 py-1.5 text-xs text-[var(--ct-fg)] cursor-pointer"
          value={sort}
          onChange={e => setSort(e.target.value as SortKey)}
        >
          <option value="recent">Recent</option>
          <option value="oldest">Oldest</option>
          <option value="importance">Importance</option>
          <option value="title">Title A-Z</option>
        </select>
      </div>

      {/* Filters */}
      <div className="flex items-center gap-1.5 px-3 py-1.5 border-b border-[var(--ct-border)] flex-wrap">
        {KINDS.map(k => {
          const active = kindFilter.has(k);
          const color = colorForKind(k);
          return (
            <button
              key={k}
              data-testid="memory-kind-filter"
              data-memory-kind={k}
              onClick={() => toggleKind(k)}
              className={`px-2 py-0.5 rounded-full text-[10px] flex items-center gap-1 select-none cursor-pointer border ${active ? "font-semibold" : "text-[var(--ct-muted-fg)] border-[var(--ct-border)] hover:border-[var(--ct-fg)]"}`}
              style={active ? { background: `${color}22`, color, borderColor: color } : undefined}
            >
              <span className="w-1.5 h-1.5 rounded-full" style={{ background: color }} />
              {k}
            </button>
          );
        })}
        <select
          data-testid="memory-area-filter"
          className="ml-1 bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-1.5 py-0.5 text-[10px] text-[var(--ct-fg)] cursor-pointer"
          value={areaFilter || ""}
          onChange={e => setAreaFilter(e.target.value || null)}
        >
          <option value="">All areas</option>
          {areas.map(a => <option key={a} value={a}>{a}</option>)}
        </select>
        <span className="ml-auto text-[10px] text-[var(--ct-muted-fg)]" data-testid="memory-list-count">{filtered.length} items</span>
      </div>

      {/* List */}
      <div ref={listRef} onScroll={handleScroll} className="flex-1 overflow-y-auto" data-testid="memory-list-results">
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
              className="border-b border-white/[0.04] hover:bg-[var(--ct-bg-hover)] transition-colors duration-75"
              data-testid="memory-list-item"
              data-memory-id={n.id}
              data-memory-kind={n.kind}
            >
              <div className="flex items-start gap-2.5 px-3 py-2 cursor-pointer" onClick={() => setEditorItemId(n.id)}>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="w-2 h-2 rounded-full shrink-0" style={{ background: color }} />
                    <span className="flex-1 text-xs text-[var(--ct-fg)] truncate font-medium">{n.title}</span>
                    <button
                      type="button"
                      data-testid="memory-list-view-graph"
                      className="text-[10px] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] shrink-0"
                      onClick={(event) => {
                        event.stopPropagation();
                        onViewInGraph(n.id);
                      }}
                    >
                      Graph
                    </button>
                    <span className="text-[10px] text-[var(--ct-muted-fg)] shrink-0 tabular-nums">{n.created_at ? relTime(n.created_at) : ""}</span>
                  </div>
                  <div className="flex items-center gap-2 pl-[18px] mt-0.5 flex-wrap">
                    <span className="px-1.5 py-px rounded-[3px] text-[10px] font-semibold" style={{ background: `${color}22`, color }}>{n.kind}</span>
                    {n.area && <><span className="text-[10px] text-[var(--ct-muted-fg)]">&middot;</span><span className="px-1.5 py-px rounded-[3px] text-[10px] bg-[var(--ct-bg-hover)] text-[var(--ct-muted-fg)]">{n.area}</span></>}
                    {(n.recurrence_count ?? 0) > 0 && (
                      <>
                        <span className="text-[10px] text-[var(--ct-muted-fg)]">&middot;</span>
                        <span className="text-[10px] px-1 py-px rounded bg-[var(--ct-accent)]/20 text-[var(--ct-accent)]" title="Times this topic has recurred">
                          {n.recurrence_count}x seen
                        </span>
                      </>
                    )}
                    {(n.access_count ?? 0) >= 3 && (
                      <>
                        <span className="text-[10px] text-[var(--ct-muted-fg)]">&middot;</span>
                        <span className="text-[10px] text-[var(--ct-muted-fg)]" title="Access count">
                          ↗ {n.access_count}
                        </span>
                      </>
                    )}
                    {(n.tags?.length ?? 0) > 0 && (
                      <>
                        <span className="text-[10px] text-[var(--ct-muted-fg)]">&middot;</span>
                        <span className="text-[10px] text-[var(--ct-muted-fg)]">{(n.tags || []).filter(t => t !== "auto-extracted").slice(0, 3).join(", ")}</span>
                      </>
                    )}
                  </div>
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
          onDeleted={(id) => { removeNode(id); onSelectItem(null); }}
          onUpdated={() => { /* graph refetch will pick up changes */ }}
        />
      )}
    </div>
  );
}

export const MemoryListTab = memo(MemoryListTabImpl);
