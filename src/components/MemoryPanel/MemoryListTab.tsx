import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ApiConfig } from "../../lib/types";
import type { MemNode } from "./types";
import { colorForKind } from "./nodeKind";
import { fetchItemFull, deleteItem, baseUrl, authHeaders } from "./memoryApi";

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

function MemoryListTabImpl({ api, nodes, selectedId, onSelectItem, onViewInGraph, removeNode }: Props) {
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState<SortKey>("recent");
  const [kindFilter, setKindFilter] = useState<Set<string>>(new Set());
  const [areaFilter, setAreaFilter] = useState<string | null>(null);
  const [expandedBody, setExpandedBody] = useState<Record<string, string>>({});
  const searchRef = useRef<HTMLInputElement>(null);

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
  }, [search, api]);

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
      // Apply filters
      if (kindFilter.size > 0) list = list.filter(n => kindFilter.has(n.kind));
      if (areaFilter) list = list.filter(n => n.area === areaFilter);
      // Sort
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

  const handleExpand = useCallback(async (id: string) => {
    if (selectedId === id) { onSelectItem(null); return; }
    onSelectItem(id);
    if (!expandedBody[id]) {
      try {
        const item = await fetchItemFull(api, id);
        setExpandedBody(prev => ({ ...prev, [id]: item.body || "(empty)" }));
      } catch { setExpandedBody(prev => ({ ...prev, [id]: "(failed to load)" })); }
    }
  }, [api, selectedId, onSelectItem, expandedBody]);

  const handleDelete = useCallback(async (id: string) => {
    try {
      await deleteItem(api, id);
      removeNode(id);
      onSelectItem(null);
    } catch { /* */ }
  }, [api, removeNode, onSelectItem]);

  const handleCopyId = useCallback((id: string) => {
    navigator.clipboard.writeText(id).catch(() => {});
  }, []);

  // Keyboard: / to focus search
  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      if (e.key === "/") { e.preventDefault(); searchRef.current?.focus(); }
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, []);

  return (
    <div className="absolute inset-0 flex flex-col">
      {/* Toolbar */}
      <div className="flex items-center gap-2 px-3 py-2 border-b border-[var(--ct-border)]">
        <div className="relative flex-1">
          <svg className="absolute left-2 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-[var(--ct-muted-fg)]" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="6.5" cy="6.5" r="5" /><line x1="10" y1="10" x2="15" y2="15" /></svg>
          <input
            ref={searchRef}
            className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded pl-7 pr-2.5 py-1.5 text-xs text-[var(--ct-fg)] placeholder:text-[var(--ct-muted-fg)] focus:outline-none focus:border-[var(--ct-accent)] font-[inherit]"
            placeholder="Search memory... (/ to focus)"
            value={search}
            onChange={e => setSearch(e.target.value)}
            onKeyDown={e => { if (e.key === "Escape") { setSearch(""); (e.target as HTMLInputElement).blur(); } }}
          />
        </div>
        <select
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
          className="ml-1 bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-1.5 py-0.5 text-[10px] text-[var(--ct-fg)] cursor-pointer"
          value={areaFilter || ""}
          onChange={e => setAreaFilter(e.target.value || null)}
        >
          <option value="">All areas</option>
          {areas.map(a => <option key={a} value={a}>{a}</option>)}
        </select>
        <span className="ml-auto text-[10px] text-[var(--ct-muted-fg)]">{filtered.length} items</span>
      </div>

      {/* List */}
      <div className="flex-1 overflow-y-auto">
        {filtered.length === 0 && (
          <div className="text-xs text-[var(--ct-muted-fg)] text-center py-8">
            {search ? `No results for "${search}"` : "No memories yet"}
          </div>
        )}
        {filtered.map(n => {
          const expanded = selectedId === n.id;
          const color = colorForKind(n.kind);
          const impBars = Math.round((n.importance ?? 3) * 2);
          return (
            <div key={n.id} className={`border-b border-white/[0.04] ${expanded ? "bg-[var(--ct-hover-bg)]" : "hover:bg-[var(--ct-hover-bg)]"} transition-colors duration-75`}>
              <div className="flex items-start gap-2.5 px-3 py-2 cursor-pointer" onClick={() => handleExpand(n.id)}>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="w-2 h-2 rounded-full shrink-0" style={{ background: color }} />
                    <span className="flex-1 text-xs text-[var(--ct-fg)] truncate font-medium">{n.title}</span>
                    <span className="text-[10px] text-[var(--ct-muted-fg)] shrink-0 tabular-nums">{n.created_at ? relTime(n.created_at) : ""}</span>
                  </div>
                  <div className="flex items-center gap-2 pl-[18px] mt-0.5 flex-wrap">
                    <span className="px-1.5 py-px rounded-[3px] text-[10px] font-semibold" style={{ background: `${color}22`, color }}>{n.kind}</span>
                    {n.area && <><span className="text-[10px] text-[var(--ct-muted-fg)]">&middot;</span><span className="px-1.5 py-px rounded-[3px] text-[10px] bg-[var(--ct-hover-bg)] text-[var(--ct-muted-fg)]">{n.area}</span></>}
                    <span className="text-[10px] text-[var(--ct-muted-fg)]">&middot;</span>
                    <span className="flex items-center gap-px h-[6px]">
                      {Array.from({ length: 10 }, (_, i) => (
                        <span key={i} className="w-[3px] h-full rounded-[1px]" style={{ background: i < impBars ? color : "rgba(255,255,255,0.08)", opacity: i < impBars ? 0.8 : 1 }} />
                      ))}
                    </span>
                    {(n.tags?.length ?? 0) > 0 && (
                      <>
                        <span className="text-[10px] text-[var(--ct-muted-fg)]">&middot;</span>
                        <span className="text-[10px] text-[var(--ct-muted-fg)]">{(n.tags || []).filter(t => t !== "auto-extracted").slice(0, 3).join(", ")}</span>
                      </>
                    )}
                  </div>
                </div>
              </div>
              {expanded && (
                <div className="mx-3 mb-2 ml-[18px] p-2.5 rounded bg-[var(--ct-bg)] border border-[var(--ct-border)]">
                  <pre className="text-[11px] font-mono text-[var(--ct-fg)] whitespace-pre-wrap leading-relaxed max-h-[160px] overflow-y-auto m-0">
                    {expandedBody[n.id] || "Loading..."}
                  </pre>
                  <div className="flex items-center gap-2 mt-2 pt-2 border-t border-[var(--ct-border)]">
                    <button onClick={() => onViewInGraph(n.id)} className="bg-transparent border border-[var(--ct-border)] text-[var(--ct-fg)] px-2 py-0.5 rounded text-[10px] cursor-pointer hover:bg-[var(--ct-hover-bg)]">Graph</button>
                    <button onClick={() => handleCopyId(n.id)} className="bg-transparent border border-[var(--ct-border)] text-[var(--ct-fg)] px-2 py-0.5 rounded text-[10px] cursor-pointer hover:bg-[var(--ct-hover-bg)]">Copy ID</button>
                    <button onClick={() => handleDelete(n.id)} className="bg-transparent border border-[#ef4444] text-[#ef4444] px-2 py-0.5 rounded text-[10px] cursor-pointer hover:bg-[#ef444422]">Delete</button>
                    <span className="ml-auto text-[10px] text-[var(--ct-muted-fg)]">{n.id}</span>
                  </div>
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

export const MemoryListTab = memo(MemoryListTabImpl);
