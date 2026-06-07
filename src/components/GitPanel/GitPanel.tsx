import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ChevronLeft, Columns2, Rows3, RefreshCw, X } from "lucide-react";
import "./styles.css";
import type { ApiConfig } from "../../lib/types";
import {
  fetchGraph,
  fetchRepos,
  NotARepoError,
  type GitCommit,
  type GitGraphResponse,
  type RepoEntry,
} from "./gitApi";
import { GraphView } from "./GraphView";
import {
  CommitDetail,
  CommitMetaHeader,
  DiffPane,
  FileList,
  useCommitFiles,
} from "./CommitDetail";
import { RepoSwitcher } from "./RepoSwitcher";
import { useGitLayout } from "./useGitLayout";
import { useSplitRatio } from "./useSplitRatio";
import { UI_BUTTON, UI_PANEL, UI_TEXT } from "../shared/design";

const PAGE = 300;

// Narrow drill-down levels: 1 commits → 2 files → 3 diff.
type NarrowLevel = 1 | 2 | 3;

interface GitPanelProps {
  api: ApiConfig;
  cwd: string | null;
  onClose: () => void;
}

export function GitPanel({ api, cwd, onClose }: GitPanelProps) {
  const { ref: rootRef, mode } = useGitLayout();
  const narrow = mode === "narrow";

  const [repos, setRepos] = useState<RepoEntry[]>([]);
  const [activeCwd, setActiveCwd] = useState<string | null>(null);

  const [graph, setGraph] = useState<GitGraphResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notRepo, setNotRepo] = useState(false);
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [selectedSha, setSelectedSha] = useState<string | null>(null);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [split, setSplit] = useState(false);
  const [level, setLevel] = useState<NarrowLevel>(1);

  // Resolve repos from the incoming cwd (may be a folder of checkouts).
  // activeCwd stays null until resolution lands so load() never hits the raw
  // folder and flashes the not-a-repo state.
  useEffect(() => {
    setActiveCwd(null);
    if (!cwd) {
      setRepos([]);
      return;
    }
    let active = true;
    const ctrl = new AbortController();
    fetchRepos(api, cwd, ctrl.signal)
      .then((rs) => {
        if (!active) return;
        setRepos(rs);
        setActiveCwd(rs.length > 0 ? rs[0].path : cwd);
      })
      .catch(() => {
        if (!active) return;
        setRepos([]);
        setActiveCwd(cwd);
      });
    return () => {
      active = false;
      ctrl.abort();
    };
  }, [api, cwd]);

  const load = useCallback(
    async (signal?: AbortSignal) => {
      if (!activeCwd) return;
      setLoading(true);
      setError(null);
      setNotRepo(false);
      try {
        const g = await fetchGraph(api, activeCwd, PAGE, 0, signal);
        setGraph(g);
        setSelectedSha(g.commits[0]?.sha ?? null);
        setSelectedPath(null);
      } catch (e) {
        if (e instanceof NotARepoError) setNotRepo(true);
        else if ((e as Error).name !== "AbortError") setError(String((e as Error).message ?? e));
      } finally {
        setLoading(false);
      }
    },
    [api, activeCwd],
  );

  useEffect(() => {
    const ctrl = new AbortController();
    setGraph(null);
    void load(ctrl.signal);
    return () => ctrl.abort();
  }, [load]);

  // Map drill level when crossing into narrow: file → 3, commit → 2, else 1.
  useEffect(() => {
    if (!narrow) return;
    setLevel(selectedPath ? 3 : selectedSha ? 2 : 1);
  }, [narrow]); // eslint-disable-line react-hooks/exhaustive-deps

  const back = useCallback(() => {
    setLevel((l) => {
      if (l === 3) {
        setSelectedPath(null);
        return 2;
      }
      if (l === 2) return 1;
      return 1;
    });
  }, []);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const inField =
        e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
      if (e.key !== "Escape" || inField) return;
      e.preventDefault();
      if (narrow && level > 1) back();
      else onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose, narrow, level, back]);

  const loadMore = useCallback(async () => {
    if (!activeCwd || !graph) return;
    setLoadingMore(true);
    try {
      const more = await fetchGraph(api, activeCwd, PAGE, graph.commits.length);
      setGraph((cur) => (cur ? { ...more, commits: [...cur.commits, ...more.commits] } : more));
    } catch {
      /* ignore — keep current page */
    } finally {
      setLoadingMore(false);
    }
  }, [api, activeCwd, graph]);

  const hasMore = graph != null && graph.commits.length % PAGE === 0 && graph.commits.length > 0;

  const onSelectCommit = useCallback(
    (sha: string) => {
      setSelectedSha(sha);
      setSelectedPath(null);
      if (narrow) setLevel(2);
    },
    [narrow],
  );

  const onSelectFile = useCallback(
    (path: string) => {
      setSelectedPath(path);
      if (narrow) setLevel(3);
    },
    [narrow],
  );

  const selectedCommit = useMemo(
    () => graph?.commits.find((c) => c.sha === selectedSha) ?? null,
    [graph, selectedSha],
  );

  const backTitle = narrow && level === 3 ? "Files" : narrow && level === 2 ? "Commits" : null;
  // Split mode makes no sense narrow — force unified there.
  const effectiveSplit = narrow ? false : split;

  return (
    <div
      ref={rootRef}
      className="ct-git absolute inset-0 flex flex-col bg-[var(--ct-bg)] pt-[var(--ct-safe-top,0px)]"
    >
      <div
        className={`${UI_PANEL.toolbar} gap-3 px-4 py-0 bg-[var(--ct-titlebar-bg)] border-b border-[var(--ct-border)] text-[var(--ct-fg)] ct-hscroll whitespace-nowrap`}
      >
        {backTitle ? (
          <button
            type="button"
            onClick={back}
            title={`Back to ${backTitle}`}
            className={`${UI_BUTTON.ghost} ${UI_BUTTON.sm} shrink-0 flex items-center gap-1 !px-2`}
          >
            <ChevronLeft size={16} />
            <span className="text-[12px]">{backTitle}</span>
          </button>
        ) : (
          <strong className="shrink-0">Git</strong>
        )}
        {repos.length > 1 && (
          <RepoSwitcher repos={repos} activePath={activeCwd ?? ""} onSelect={setActiveCwd} />
        )}
        {graph?.branch && repos.length <= 1 && (
          <span className="text-[11px] text-[var(--ct-green)] shrink-0">{graph.branch}</span>
        )}
        {graph?.dirty && (
          <span className="text-[11px] text-[var(--ct-yellow)] shrink-0">uncommitted changes</span>
        )}
        <span className="text-[11px] opacity-60 truncate min-w-0 flex-1" title={activeCwd ?? ""}>
          {activeCwd}
        </span>
        {!narrow && (
          <button
            type="button"
            onClick={() => setSplit((v) => !v)}
            title={split ? "Unified view" : "Split view"}
            className={`${UI_BUTTON.icon} ${UI_BUTTON.sm} shrink-0`}
          >
            {split ? <Rows3 size={14} /> : <Columns2 size={14} />}
          </button>
        )}
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
        {!activeCwd && <Centered text="No repository selected." />}
        {activeCwd && notRepo && <Centered text="Not a git repository." />}
        {activeCwd && error && (
          <div className="absolute top-4 left-1/2 -translate-x-1/2 px-3 py-2 rounded bg-[var(--ct-red)] text-white text-xs z-10">
            {error}
          </div>
        )}
        {activeCwd && loading && !graph && <Centered text="Loading commit graph…" />}
        {activeCwd && graph && !notRepo && (
          narrow ? (
            <NarrowBody
              api={api}
              cwd={activeCwd}
              graph={graph}
              level={level}
              selectedSha={selectedSha}
              selectedPath={selectedPath}
              selectedCommit={selectedCommit}
              hasMore={hasMore}
              loadingMore={loadingMore}
              onLoadMore={() => void loadMore()}
              onSelectCommit={onSelectCommit}
              onSelectFile={onSelectFile}
            />
          ) : (
            <WideBody
              api={api}
              cwd={activeCwd}
              graph={graph}
              selectedSha={selectedSha}
              split={effectiveSplit}
              hasMore={hasMore}
              loadingMore={loadingMore}
              onLoadMore={() => void loadMore()}
              onSelectCommit={onSelectCommit}
            />
          )
        )}
      </div>
    </div>
  );
}

interface WideBodyProps {
  api: ApiConfig;
  cwd: string;
  graph: GitGraphResponse;
  selectedSha: string | null;
  split: boolean;
  hasMore: boolean;
  loadingMore: boolean;
  onLoadMore: () => void;
  onSelectCommit: (sha: string) => void;
}

function WideBody({
  api,
  cwd,
  graph,
  selectedSha,
  split,
  hasMore,
  loadingMore,
  onLoadMore,
  onSelectCommit,
}: WideBodyProps) {
  const boxRef = useRef<HTMLDivElement>(null);
  const { ratio, onPointerDown, reset } = useSplitRatio(boxRef);

  return (
    <div ref={boxRef} className="absolute inset-0 flex min-h-0">
      <div className="relative min-w-0" style={{ flex: `0 0 ${ratio * 100}%` }}>
        <GraphView
          commits={graph.commits}
          selectedSha={selectedSha}
          onSelect={onSelectCommit}
          hasMore={hasMore}
          loadingMore={loadingMore}
          onLoadMore={onLoadMore}
        />
      </div>
      <div
        className="ct-git-divider"
        onPointerDown={onPointerDown}
        onDoubleClick={reset}
        role="separator"
        aria-orientation="vertical"
      />
      <div className="flex-1 min-w-0">
        {selectedSha ? (
          <CommitDetail api={api} cwd={cwd} sha={selectedSha} split={split} />
        ) : (
          <Centered text="Select a commit." />
        )}
      </div>
    </div>
  );
}

interface NarrowBodyProps {
  api: ApiConfig;
  cwd: string;
  graph: GitGraphResponse;
  level: NarrowLevel;
  selectedSha: string | null;
  selectedPath: string | null;
  selectedCommit: GitCommit | null;
  hasMore: boolean;
  loadingMore: boolean;
  onLoadMore: () => void;
  onSelectCommit: (sha: string) => void;
  onSelectFile: (path: string) => void;
}

function NarrowBody({
  api,
  cwd,
  graph,
  level,
  selectedSha,
  selectedPath,
  selectedCommit,
  hasMore,
  loadingMore,
  onLoadMore,
  onSelectCommit,
  onSelectFile,
}: NarrowBodyProps) {
  if (level === 1) {
    return (
      <div className="absolute inset-0 pb-[var(--ct-safe-bottom,0px)]">
        <GraphView
          commits={graph.commits}
          selectedSha={selectedSha}
          onSelect={onSelectCommit}
          hasMore={hasMore}
          loadingMore={loadingMore}
          onLoadMore={onLoadMore}
          touch
        />
      </div>
    );
  }
  if (level === 2) {
    return selectedSha && selectedCommit ? (
      <NarrowFiles
        api={api}
        cwd={cwd}
        sha={selectedSha}
        commit={selectedCommit}
        selectedPath={selectedPath}
        onSelectFile={onSelectFile}
      />
    ) : (
      <Centered text="Select a commit." />
    );
  }
  return selectedSha && selectedPath ? (
    <div className="absolute inset-0 overflow-auto pb-[var(--ct-safe-bottom,0px)]">
      <DiffPane api={api} cwd={cwd} sha={selectedSha} path={selectedPath} split={false} />
    </div>
  ) : (
    <Centered text="Select a file." />
  );
}

interface NarrowFilesProps {
  api: ApiConfig;
  cwd: string;
  sha: string;
  commit: GitCommit;
  selectedPath: string | null;
  onSelectFile: (path: string) => void;
}

function NarrowFiles({ api, cwd, sha, commit, selectedPath, onSelectFile }: NarrowFilesProps) {
  const { files, error, loading } = useCommitFiles(api, cwd, sha);
  return (
    <div className="absolute inset-0 overflow-auto pb-[var(--ct-safe-bottom,0px)]">
      <CommitMetaHeader commit={commit} />
      <FileList
        files={files}
        error={error}
        loading={loading}
        selectedPath={selectedPath}
        onSelect={onSelectFile}
        touch
      />
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
