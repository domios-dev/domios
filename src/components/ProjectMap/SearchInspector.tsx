import { useEffect, useMemo, useRef, useState } from "react";
import { Search, X, Loader2 } from "lucide-react";
import type { ApiConfig } from "../../lib/types";
import { useMapSearch, type SearchHit } from "./hooks/useMapSearch";
import type { MapProgress } from "./hooks/useMapProgress";
import { colorForKind } from "./styles";

interface Props {
  api: ApiConfig;
  workspaceId: string | undefined;
  onPick: (id: number) => void;
  focusId: number | null;
  progress: MapProgress | null;
}

const KIND_FILTERS: { value: string; label: string }[] = [
  { value: "", label: "all" },
  { value: "function", label: "fn" },
  { value: "method", label: "method" },
  { value: "class", label: "class" },
  { value: "file", label: "file" },
];

export function SearchInspector({ api, workspaceId, onPick, focusId, progress }: Props) {
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState("");
  const [semantic, setSemantic] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  const { hits, loading, error } = useMapSearch(api, workspaceId, query, {
    kind: kind || undefined,
    semanticOnly: semantic,
    limit: 50,
  });

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault();
        inputRef.current?.focus();
        inputRef.current?.select();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const grouped = useMemo(() => groupHitsByPath(hits), [hits]);

  const showHelp = query.trim().length === 0;

  return (
    <div className="absolute inset-0 flex flex-col bg-[var(--ct-bg)]">
      <div className="px-3 py-2 border-b border-[var(--ct-border)] flex flex-col gap-2">
        <div className="flex items-center gap-2">
          <div className="relative flex-1">
            <Search size={13} className="absolute left-2 top-1/2 -translate-y-1/2 text-[var(--ct-muted)]" />
            <input
              ref={inputRef}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search functions, classes, files…  (⌘K)"
              className="w-full pl-7 pr-7 py-1.5 text-[12px] bg-[var(--ct-active)]/30 border border-[var(--ct-border)] rounded-sm font-mono text-[var(--ct-fg)] outline-none focus:border-[var(--ct-accent)] placeholder:text-[var(--ct-muted)]"
            />
            {query && (
              <button
                onClick={() => setQuery("")}
                className="absolute right-2 top-1/2 -translate-y-1/2 text-[var(--ct-muted)] hover:text-[var(--ct-fg)]"
              >
                <X size={12} />
              </button>
            )}
          </div>
          {loading && <Loader2 size={12} className="text-[var(--ct-muted)] animate-spin" />}
        </div>
        <div className="flex items-center gap-1 text-[10px]">
          {KIND_FILTERS.map((f) => (
            <button
              key={f.value}
              onClick={() => setKind(f.value)}
              className={`px-1.5 py-0.5 rounded-sm border transition-colors ${
                kind === f.value
                  ? "bg-[var(--ct-accent)]/20 border-[var(--ct-accent)]/40 text-[var(--ct-accent)]"
                  : "border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"
              }`}
            >
              {f.label}
            </button>
          ))}
          <div className="flex-1" />
          <label className="flex items-center gap-1 cursor-pointer text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]">
            <input
              type="checkbox"
              checked={semantic}
              onChange={(e) => setSemantic(e.target.checked)}
              className="accent-[var(--ct-accent)]"
            />
            <span>semantic only</span>
          </label>
        </div>
      </div>

      <div className="flex-1 min-h-0 overflow-y-auto">
        {progress?.running && (
          <div className="px-3 py-1.5 text-[10px] text-[var(--ct-yellow)] bg-[var(--ct-yellow)]/10 border-b border-[var(--ct-yellow)]/20">
            Indexing in progress — results may be incomplete until {progress.total > 0 ? Math.round((progress.done / progress.total) * 100) : 0}% reaches 100%.
          </div>
        )}
        {error && (
          <div className="px-3 py-2 text-[11px] text-[var(--ct-red)]">{error}</div>
        )}
        {showHelp && <SearchHelp />}
        {!showHelp && hits.length === 0 && !loading && (
          <div className="px-3 py-6 text-[11px] text-[var(--ct-muted)] italic">
            No matches. Try different terms or toggle "semantic only".
          </div>
        )}
        {!showHelp && grouped.map(({ path, items }) => (
          <ResultGroup
            key={path ?? "__nopath"}
            path={path}
            items={items}
            onPick={onPick}
            focusId={focusId}
          />
        ))}
      </div>

      <div className="px-3 py-1.5 border-t border-[var(--ct-border)] text-[9px] text-[var(--ct-muted)] flex items-center justify-between">
        <span>{hits.length > 0 ? `${hits.length} result${hits.length === 1 ? "" : "s"}` : ""}</span>
        <span>↵ open · Esc close · ⌘K focus</span>
      </div>
    </div>
  );
}

function ResultGroup({
  path,
  items,
  onPick,
  focusId,
}: {
  path: string | null;
  items: SearchHit[];
  onPick: (id: number) => void;
  focusId: number | null;
}) {
  return (
    <div className="border-b border-[var(--ct-border)] py-1">
      <div className="px-3 py-1 text-[10px] font-mono text-[var(--ct-muted-fg)] truncate" title={path ?? ""}>
        {path ?? "(no path)"}
      </div>
      {items.map((hit) => (
        <button
          key={hit.id}
          onClick={() => onPick(hit.id)}
          className={`w-full text-left px-3 py-1 flex items-center gap-2 hover:bg-[var(--ct-hover)] transition-colors ${
            focusId === hit.id ? "bg-[var(--ct-active)]/40" : ""
          }`}
        >
          <span
            className="inline-block w-1.5 h-1.5 rounded-full shrink-0"
            style={{ background: colorForKind(hit.kind) }}
          />
          <span className="text-[9px] uppercase text-[var(--ct-muted)] w-12 shrink-0">{hit.kind}</span>
          <span className="font-mono text-[11px] text-[var(--ct-fg)] truncate flex-1">{hit.display_name}</span>
          {hit.start_line != null && (
            <span className="text-[9px] text-[var(--ct-muted)]">:{hit.start_line + 1}</span>
          )}
        </button>
      ))}
    </div>
  );
}

function SearchHelp() {
  return (
    <div className="px-3 py-4 text-[11px] text-[var(--ct-muted-fg)] flex flex-col gap-3">
      <div>
        <div className="text-[var(--ct-fg)] mb-1">Quick examples</div>
        <ul className="flex flex-col gap-0.5 ml-2">
          <li><code className="text-[var(--ct-accent)]">spawn_agent</code> — substring match across symbols</li>
          <li><code className="text-[var(--ct-accent)]">workspace</code> — every workspace-related def</li>
          <li><code className="text-[var(--ct-accent)]">how does the agent get spawned</code> + semantic — concept search</li>
        </ul>
      </div>
      <div className="text-[10px] text-[var(--ct-muted)]">
        Filter by kind with the chips above. Semantic-only flips to vector search.
      </div>
    </div>
  );
}

function groupHitsByPath(hits: SearchHit[]): Array<{ path: string | null; items: SearchHit[] }> {
  const map = new Map<string, SearchHit[]>();
  const order: string[] = [];
  for (const h of hits) {
    const key = h.path ?? "__nopath";
    if (!map.has(key)) {
      map.set(key, []);
      order.push(key);
    }
    map.get(key)!.push(h);
  }
  return order.map((key) => ({
    path: key === "__nopath" ? null : key,
    items: map.get(key)!,
  }));
}
