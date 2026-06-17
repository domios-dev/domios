import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ChevronLeft, Columns2, Rows3, RefreshCw, X } from "lucide-react";
import "./app.css";
import type { ApiConfig } from "./types";
import {
  fetchGraph,
  fetchRepos,
  NotARepoError,
  WORKDIR_SHA,
  type GitCommit,
  type GitGraphResponse,
  type RepoEntry,
} from "./gitApi";
import { GraphView } from "./GraphView";
import {
  CommitFilesPane,
  CommitMetaHeader,
  DiffPane,
  FileList,
  FileListLoadingBadge,
  useCommitFiles,
  useCopyPathMenu,
} from "./CommitDetail";
import { WorkdirFilesPane, WorkdirFilesWithDiff } from "./WorkdirDetail";
import { DiffModal } from "./DiffModal";
import { RepoSwitcher } from "./RepoSwitcher";
import { useGitLayout, type GitLayoutMode } from "./useGitLayout";
import { useDragRatio } from "./useDragRatio";
import { UI_BUTTON, UI_PANEL, UI_TEXT } from "./design";

const PAGE = 300;
const AUTO_REFRESH_MS = 10_000;

// Narrow drill-down levels: 1 commits → 2 files → 3 diff (DiffModal fullscreen).
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
  const [nextRefreshAt, setNextRefreshAt] = useState<number | null>(null);
  const [now, setNow] = useState(() => Date.now());
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
        setSelectedSha((cur) => {
          if (cur === WORKDIR_SHA && (g.branch || g.dirty)) return cur;
          if (cur && g.commits.some((commit) => commit.sha === cur)) return cur;
          return g.branch || g.dirty ? WORKDIR_SHA : (g.commits[0]?.sha ?? null);
        });
        setNextRefreshAt(Date.now() + AUTO_REFRESH_MS);
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
    setSelectedPath(null);
    void load(ctrl.signal);
    return () => ctrl.abort();
  }, [load]);

  useEffect(() => {
    if (!activeCwd) return;
    const interval = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(interval);
  }, [activeCwd]);

  useEffect(() => {
    if (!activeCwd || nextRefreshAt == null || loading) return;
    if (now < nextRefreshAt) return;
    void load();
  }, [activeCwd, load, loading, nextRefreshAt, now]);

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
      return 1;
    });
  }, []);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const inField =
        e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
      if (e.key !== "Escape" || inField) return;
      // Medium-mode modal owns its own Escape; only close panel here when no
      // overlay/drill is active.
      if (mode === "medium" && selectedPath) return;
      e.preventDefault();
      if (narrow && level > 1) back();
      else onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose, narrow, level, back, mode, selectedPath]);

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

  // After a workdir mutation: refetch the first graph page so newly created
  // commits appear immediately. If the tree is clean now, move from the
  // workdir pseudo-row to HEAD.
  const onReverted = useCallback(async () => {
    if (!activeCwd) return;
    try {
      const g = await fetchGraph(api, activeCwd, PAGE, 0);
      setGraph(g);
      setNextRefreshAt(Date.now() + AUTO_REFRESH_MS);
      if (!g.dirty) {
        setSelectedSha((cur) =>
          cur === WORKDIR_SHA && !g.branch ? (g.commits[0]?.sha ?? null) : cur,
        );
        setSelectedPath(null);
      }
    } catch {
      /* ignore — keep current graph */
    }
  }, [api, activeCwd]);

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

  const closeDiff = useCallback(() => {
    setSelectedPath(null);
    if (narrow) setLevel(2);
  }, [narrow]);

  const selectedCommit = useMemo(
    () => graph?.commits.find((c) => c.sha === selectedSha) ?? null,
    [graph, selectedSha],
  );

  const backTitle = narrow && level === 3 ? "Files" : narrow && level === 2 ? "Commits" : null;
  // Split mode is meaningless on the narrow fullscreen diff — force unified there.
  const effectiveSplit = narrow ? false : split;
  const refreshCountdown =
    nextRefreshAt == null ? 10 : Math.max(0, Math.ceil((nextRefreshAt - now) / 1000));

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
        <BranchDelta
          base={graph?.branch_base ?? null}
          added={graph?.branch_added ?? null}
          deleted={graph?.branch_deleted ?? null}
        />
        <span className="text-[11px] opacity-60 truncate min-w-0 flex-1" title={activeCwd ?? ""}>
          {activeCwd}
        </span>
        {mode === "wide" && (
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
          title={`Refresh now. Auto-refresh in ${refreshCountdown}s`}
          className={`${UI_BUTTON.ghost} ${UI_BUTTON.sm} ct-git-refresh-pill shrink-0`}
        >
          <RefreshCw size={14} />
          <span>{loading && graph ? "…" : `${refreshCountdown}s`}</span>
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
          <PanelBody
            api={api}
            cwd={activeCwd}
            mode={mode}
            graph={graph}
            level={level}
            selectedSha={selectedSha}
            selectedPath={selectedPath}
            selectedCommit={selectedCommit}
            split={effectiveSplit}
            onToggleSplit={() => setSplit((v) => !v)}
            hasMore={hasMore}
            loadingMore={loadingMore}
            onLoadMore={() => void loadMore()}
            onSelectCommit={onSelectCommit}
            onSelectFile={onSelectFile}
            onCloseDiff={closeDiff}
            onReverted={onReverted}
          />
        )}
      </div>
    </div>
  );
}

interface PanelBodyProps {
  api: ApiConfig;
  cwd: string;
  mode: GitLayoutMode;
  graph: GitGraphResponse;
  level: NarrowLevel;
  selectedSha: string | null;
  selectedPath: string | null;
  selectedCommit: GitCommit | null;
  split: boolean;
  onToggleSplit: () => void;
  hasMore: boolean;
  loadingMore: boolean;
  onLoadMore: () => void;
  onSelectCommit: (sha: string) => void;
  onSelectFile: (path: string) => void;
  onCloseDiff: () => void;
  onReverted: () => void;
}

function PanelBody(props: PanelBodyProps) {
  if (props.mode === "wide") return <WideBody {...props} />;
  if (props.mode === "narrow") return <NarrowBody {...props} />;
  return <MediumBody {...props} />;
}

function WideBody({
  api,
  cwd,
  graph,
  selectedSha,
  selectedPath,
  selectedCommit,
  split,
  hasMore,
  loadingMore,
  onLoadMore,
  onSelectCommit,
  onSelectFile,
  onReverted,
}: PanelBodyProps) {
  const outerRef = useRef<HTMLDivElement>(null);
  const filesTreeRef = useRef<HTMLDivElement>(null);
  const previewSplit = useDragRatio(outerRef, {
    storageKey: "ct-git-main-preview-split",
    axis: "x",
    minPx: 300,
    defaultRatio: 0.4,
  });
  const filesTreeSplit = useDragRatio(filesTreeRef, {
    storageKey: "ct-git-files-tree-split",
    axis: "y",
    minPx: 150,
    defaultRatio: 0.42,
  });

  const isWorkdir = selectedSha === WORKDIR_SHA;

  return (
    <div ref={outerRef} className="absolute inset-0 flex min-h-0">
      <div
        ref={filesTreeRef}
        className="relative flex flex-col min-w-0"
        style={{ flex: `0 0 ${previewSplit.ratio * 100}%` }}
      >
        <div
          className="relative min-h-0 overflow-hidden"
          style={{ flex: `0 0 ${filesTreeSplit.ratio * 100}%` }}
        >
          {isWorkdir ? (
            <WorkdirFilesPane
              api={api}
              cwd={cwd}
              selectedPath={selectedPath}
              onSelectFile={onSelectFile}
              onReverted={onReverted}
            />
          ) : selectedSha && selectedCommit ? (
            <CommitFilesPane
              api={api}
              cwd={cwd}
              sha={selectedSha}
              commit={selectedCommit}
              selectedPath={selectedPath}
              onSelectFile={onSelectFile}
            />
          ) : (
            <Centered text="Select a commit." />
          )}
        </div>
        <div
          className="ct-git-divider ct-git-divider--h"
          onPointerDown={filesTreeSplit.onPointerDown}
          onDoubleClick={filesTreeSplit.reset}
          role="separator"
          aria-orientation="horizontal"
        />
        <div className="relative flex-1 min-h-0">
          <GraphView
            commits={graph.commits}
            selectedSha={selectedSha}
            onSelect={onSelectCommit}
            hasMore={hasMore}
            loadingMore={loadingMore}
            onLoadMore={onLoadMore}
            dirty={graph.dirty}
            headSha={graph.head_sha}
            branch={graph.branch}
          />
        </div>
      </div>
      <div
        className="ct-git-divider"
        onPointerDown={previewSplit.onPointerDown}
        onDoubleClick={previewSplit.reset}
        role="separator"
        aria-orientation="vertical"
      />
      <div className="relative flex-1 min-w-0 overflow-hidden">
        {selectedSha && selectedPath ? (
          <DiffPane api={api} cwd={cwd} sha={selectedSha} path={selectedPath} split={split} />
        ) : selectedSha ? (
          <Centered text="Select a file to view its diff." />
        ) : (
          <Centered text="Select a commit." />
        )}
      </div>
    </div>
  );
}

// Medium: graph | v-divider | file list (full height). File click → DiffModal.
function MediumBody({
  api,
  cwd,
  graph,
  selectedSha,
  selectedPath,
  selectedCommit,
  split,
  onToggleSplit,
  hasMore,
  loadingMore,
  onLoadMore,
  onSelectCommit,
  onSelectFile,
  onCloseDiff,
  onReverted,
}: PanelBodyProps) {
  const boxRef = useRef<HTMLDivElement>(null);
  const { ratio, onPointerDown, reset } = useDragRatio(boxRef, {
    storageKey: "ct-git-split",
    axis: "x",
    minPx: 240,
  });

  const isWorkdir = selectedSha === WORKDIR_SHA;

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
          dirty={graph.dirty}
          headSha={graph.head_sha}
          branch={graph.branch}
        />
      </div>
      <div
        className="ct-git-divider"
        onPointerDown={onPointerDown}
        onDoubleClick={reset}
        role="separator"
        aria-orientation="vertical"
      />
      <div className="relative flex-1 min-w-0">
        {isWorkdir ? (
          <WorkdirFilesWithDiff
            api={api}
            cwd={cwd}
            selectedPath={selectedPath}
            split={split}
            onToggleSplit={onToggleSplit}
            onSelectFile={onSelectFile}
            onCloseDiff={onCloseDiff}
            onReverted={onReverted}
          />
        ) : selectedSha && selectedCommit ? (
          <FilesWithDiff
            api={api}
            cwd={cwd}
            sha={selectedSha}
            commit={selectedCommit}
            selectedPath={selectedPath}
            split={split}
            onToggleSplit={onToggleSplit}
            onSelectFile={onSelectFile}
            onCloseDiff={onCloseDiff}
          />
        ) : (
          <Centered text="Select a commit." />
        )}
      </div>
    </div>
  );
}

// Narrow: drill-down 1→2→3. Level 3 reuses DiffModal in fullscreen variant.
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
  onCloseDiff,
  onReverted,
}: PanelBodyProps) {
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
          dirty={graph.dirty}
          headSha={graph.head_sha}
          branch={graph.branch}
          touch
        />
      </div>
    );
  }
  if (selectedSha === WORKDIR_SHA) {
    return (
      <WorkdirFilesWithDiff
        api={api}
        cwd={cwd}
        selectedPath={level === 3 ? selectedPath : null}
        split={false}
        onToggleSplit={() => {}}
        onSelectFile={onSelectFile}
        onCloseDiff={onCloseDiff}
        onReverted={onReverted}
        fullscreen
        touch
      />
    );
  }
  if (selectedSha && selectedCommit) {
    return (
      <FilesWithDiff
        api={api}
        cwd={cwd}
        sha={selectedSha}
        commit={selectedCommit}
        selectedPath={level === 3 ? selectedPath : null}
        split={false}
        onToggleSplit={() => {}}
        onSelectFile={onSelectFile}
        onCloseDiff={onCloseDiff}
        fullscreen
        touch
      />
    );
  }
  return <Centered text="Select a commit." />;
}

interface FilesWithDiffProps {
  api: ApiConfig;
  cwd: string;
  sha: string;
  commit: GitCommit;
  selectedPath: string | null;
  split: boolean;
  onToggleSplit: () => void;
  onSelectFile: (path: string) => void;
  onCloseDiff: () => void;
  fullscreen?: boolean;
  touch?: boolean;
}

// File list (medium/narrow) with the DiffModal layered over it when a file is
// selected. Shared so medium and narrow drive the exact same diff component.
function FilesWithDiff({
  api,
  cwd,
  sha,
  commit,
  selectedPath,
  split,
  onToggleSplit,
  onSelectFile,
  onCloseDiff,
  fullscreen,
  touch,
}: FilesWithDiffProps) {
  const { files, error, loading } = useCommitFiles(api, cwd, sha);
  const copyMenu = useCopyPathMenu();
  const selectedFile = useMemo(
    () => files.find((f) => f.path === selectedPath) ?? null,
    [files, selectedPath],
  );

  return (
    <div className="absolute inset-0 overflow-auto pb-[var(--ct-safe-bottom,0px)]">
      <CommitMetaHeader commit={commit} />
      <FileList
        files={files}
        error={error}
        loading={loading}
        selectedPath={selectedPath}
        onSelect={onSelectFile}
        onContextMenu={copyMenu.open}
        touch={touch}
      />
      <FileListLoadingBadge loading={loading} />
      {copyMenu.node}
      {selectedFile && (
        <DiffModal
          api={api}
          cwd={cwd}
          sha={sha}
          file={selectedFile}
          split={split}
          onToggleSplit={onToggleSplit}
          onClose={onCloseDiff}
          fullscreen={fullscreen}
        />
      )}
    </div>
  );
}

// Net line delta of the branch vs its fork point from the default branch (working tree included).
function BranchDelta({
  base,
  added,
  deleted,
}: {
  base: string | null;
  added: number | null;
  deleted: number | null;
}) {
  if (added == null || deleted == null || (added === 0 && deleted === 0)) return null;
  const fmt = (n: number) => n.toLocaleString();
  return (
    <span
      className="ct-git-delta shrink-0"
      title={`Lines changed on this branch vs ${base ?? "base"} (working tree included)`}
    >
      <span className="ct-git-delta-add">+{fmt(added)}</span>
      <span className="ct-git-delta-del">−{fmt(deleted)}</span>
    </span>
  );
}

function Centered({ text }: { text: string }) {
  return (
    <div className="absolute inset-0 flex items-center justify-center">
      <span className={UI_TEXT.meta}>{text}</span>
    </div>
  );
}
