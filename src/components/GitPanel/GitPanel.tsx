import { useCallback, useEffect, useState } from "react";
import { Columns2, Rows3, RefreshCw, X } from "lucide-react";
import "./styles.css";
import type { ApiConfig } from "../../lib/types";
import { fetchGraph, NotARepoError, type GitGraphResponse } from "./gitApi";
import { GraphView } from "./GraphView";
import { CommitDetail } from "./CommitDetail";
import { UI_BUTTON, UI_PANEL, UI_TEXT } from "../shared/design";

const PAGE = 300;

interface GitPanelProps {
  api: ApiConfig;
  cwd: string | null;
  onClose: () => void;
}

export function GitPanel({ api, cwd, onClose }: GitPanelProps) {
  const [graph, setGraph] = useState<GitGraphResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notRepo, setNotRepo] = useState(false);
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [selectedSha, setSelectedSha] = useState<string | null>(null);
  const [split, setSplit] = useState(false);

  const load = useCallback(
    async (signal?: AbortSignal) => {
      if (!cwd) return;
      setLoading(true);
      setError(null);
      setNotRepo(false);
      try {
        const g = await fetchGraph(api, cwd, PAGE, 0, signal);
        setGraph(g);
        setSelectedSha(g.commits[0]?.sha ?? null);
      } catch (e) {
        if (e instanceof NotARepoError) setNotRepo(true);
        else if ((e as Error).name !== "AbortError") setError(String((e as Error).message ?? e));
      } finally {
        setLoading(false);
      }
    },
    [api, cwd],
  );

  useEffect(() => {
    const ctrl = new AbortController();
    setGraph(null);
    void load(ctrl.signal);
    return () => ctrl.abort();
  }, [load]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const inField =
        e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
      if (e.key === "Escape" && !inField) {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  const loadMore = useCallback(async () => {
    if (!cwd || !graph) return;
    setLoadingMore(true);
    try {
      const more = await fetchGraph(api, cwd, PAGE, graph.commits.length);
      setGraph((cur) =>
        cur ? { ...more, commits: [...cur.commits, ...more.commits] } : more,
      );
    } catch {
      /* ignore — keep current page */
    } finally {
      setLoadingMore(false);
    }
  }, [api, cwd, graph]);

  const hasMore = graph != null && graph.commits.length % PAGE === 0 && graph.commits.length > 0;

  return (
    <div className="ct-git absolute inset-0 flex flex-col bg-[var(--ct-bg)] pt-[var(--ct-safe-top,0px)]">
      <div
        className={`${UI_PANEL.toolbar} gap-3 px-4 py-0 bg-[var(--ct-titlebar-bg)] border-b border-[var(--ct-border)] text-[var(--ct-fg)] ct-hscroll whitespace-nowrap`}
      >
        <strong className="shrink-0">Git</strong>
        {graph?.branch && (
          <span className="text-[11px] text-[var(--ct-green)] shrink-0">{graph.branch}</span>
        )}
        {graph?.dirty && (
          <span className="text-[11px] text-[var(--ct-yellow)] shrink-0">uncommitted changes</span>
        )}
        <span className="text-[11px] opacity-60 truncate min-w-0 flex-1" title={cwd ?? ""}>
          {cwd}
        </span>
        <button
          type="button"
          onClick={() => setSplit((v) => !v)}
          title={split ? "Unified view" : "Split view"}
          className={`${UI_BUTTON.icon} ${UI_BUTTON.sm} shrink-0`}
        >
          {split ? <Rows3 size={14} /> : <Columns2 size={14} />}
        </button>
        <button
          type="button"
          onClick={() => void load()}
          title="Refresh"
          className={`${UI_BUTTON.icon} ${UI_BUTTON.sm} shrink-0`}
        >
          <RefreshCw size={14} />
        </button>
        <button
          type="button"
          onClick={onClose}
          title="Close (Esc)"
          className={`${UI_BUTTON.icon} ${UI_BUTTON.sm} shrink-0`}
        >
          <X size={14} />
        </button>
      </div>

      <div className="relative flex-1 min-h-0">
        {!cwd && <Centered text="No repository selected." />}
        {cwd && notRepo && <Centered text="Not a git repository." />}
        {cwd && error && (
          <div className="absolute top-4 left-1/2 -translate-x-1/2 px-3 py-2 rounded bg-[var(--ct-red)] text-white text-xs z-10">
            {error}
          </div>
        )}
        {cwd && loading && !graph && <Centered text="Loading commit graph…" />}
        {cwd && graph && !notRepo && (
          <div className="absolute inset-0 flex min-h-0">
            <div className="relative w-1/2 min-w-[320px] border-r border-[var(--ct-border)]">
              <GraphView
                commits={graph.commits}
                selectedSha={selectedSha}
                onSelect={setSelectedSha}
                hasMore={hasMore}
                loadingMore={loadingMore}
                onLoadMore={() => void loadMore()}
              />
            </div>
            <div className="flex-1 min-w-0">
              {selectedSha ? (
                <CommitDetail api={api} cwd={cwd} sha={selectedSha} split={split} />
              ) : (
                <Centered text="Select a commit." />
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function Centered({ text }: { text: string }) {
  return (
    <div className="absolute inset-0 flex items-center justify-center">
      <span className={UI_TEXT.meta}>{text}</span>
    </div>
  );
}
