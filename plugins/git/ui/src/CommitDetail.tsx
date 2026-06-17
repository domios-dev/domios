import { useEffect, useMemo, useRef, useState } from "react";
import { DiffView, DiffModeEnum } from "@git-diff-view/react";
import "@git-diff-view/react/styles/diff-view.css";
import type { ApiConfig } from "./types";
import {
  fetchCommitDiff,
  fetchCommitFiles,
  fetchWorkdirDiff,
  type GitCommit,
  type GitCommitFile,
  WORKDIR_SHA,
} from "./gitApi";
import { useDragRatio } from "./useDragRatio";
import { ContextMenu } from "./ContextMenu";
import { UI_TEXT } from "./design";

const diffScrollPositions = new Map<string, { top: number; left: number }>();
const MAX_DIFF_SCROLL_POSITIONS = 200;

function rememberDiffScroll(key: string, top: number, left: number) {
  if (diffScrollPositions.size >= MAX_DIFF_SCROLL_POSITIONS && !diffScrollPositions.has(key)) {
    const oldest = diffScrollPositions.keys().next().value;
    if (oldest) diffScrollPositions.delete(oldest);
  }
  diffScrollPositions.set(key, { top, left });
}

// Right-click "Copy path" menu for commit file rows (no revert on history).
export function useCopyPathMenu() {
  const [menu, setMenu] = useState<{ x: number; y: number; path: string } | null>(null);
  const open = (e: React.MouseEvent, file: DisplayFile) => {
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY, path: file.path });
  };
  const close = () => setMenu(null);
  const node = menu ? (
    <ContextMenu
      x={menu.x}
      y={menu.y}
      actions={[{ label: "Copy path", onClick: () => void navigator.clipboard?.writeText(menu.path) }]}
      onClose={close}
    />
  ) : null;
  return { open, node };
}

// Display shape shared by commit files and workdir files. Counts are nullable
// because untracked/binary entries have no numstat.
export interface DisplayFile {
  path: string;
  old_path?: string | null;
  added: number | null;
  deleted: number | null;
  status?: string;
  staged?: boolean;
}

interface CommitDetailProps {
  api: ApiConfig;
  cwd: string;
  sha: string;
  split: boolean;
  selectedPath: string | null;
  onSelectFile: (path: string) => void;
}

// Wide-layout detail: file list on the left, diff preview on the right.
// Selection is controlled by the panel so it survives layout-mode crossings.
export function CommitDetail({ api, cwd, sha, split, selectedPath, onSelectFile }: CommitDetailProps) {
  const { files, error, loading } = useCommitFiles(api, cwd, sha);
  const copyMenu = useCopyPathMenu();
  const boxRef = useRef<HTMLDivElement>(null);
  const { ratio, onPointerDown, reset } = useDragRatio(boxRef, {
    storageKey: "ct-git-commit-files-split",
    axis: "x",
    minPx: 220,
    defaultRatio: 0.32,
  });

  // Wide always shows a diff: fall back to the first file for display only,
  // without writing panel state (so leaving wide doesn't auto-open the modal).
  const effectivePath = selectedPath ?? files[0]?.path ?? null;

  return (
    <div ref={boxRef} className="flex h-full min-h-0">
      <div className="relative overflow-auto min-w-0" style={{ flex: `0 0 ${ratio * 100}%` }}>
        <FileList
          files={files}
          error={error}
          loading={loading}
          selectedPath={effectivePath}
          onSelect={onSelectFile}
          onContextMenu={copyMenu.open}
        />
        <FileListLoadingBadge loading={loading} />
        {copyMenu.node}
      </div>
      <div
        className="ct-git-divider"
        onPointerDown={onPointerDown}
        onDoubleClick={reset}
        role="separator"
        aria-orientation="vertical"
      />
      <div className="flex-1 min-w-0 overflow-hidden">
        {effectivePath ? (
          <DiffPane api={api} cwd={cwd} sha={sha} path={effectivePath} split={split} />
        ) : (
          <div className={`p-4 ${UI_TEXT.meta}`}>Select a file to view its diff.</div>
        )}
      </div>
    </div>
  );
}

interface CommitFilesPaneProps {
  api: ApiConfig;
  cwd: string;
  sha: string;
  commit?: GitCommit | null;
  selectedPath: string | null;
  onSelectFile: (path: string) => void;
  touch?: boolean;
}

export function CommitFilesPane({
  api,
  cwd,
  sha,
  commit,
  selectedPath,
  onSelectFile,
  touch,
}: CommitFilesPaneProps) {
  const { files, error, loading } = useCommitFiles(api, cwd, sha);
  const copyMenu = useCopyPathMenu();

  useEffect(() => {
    if (loading || files.length === 0 || files.some((f) => f.path === selectedPath)) return;
    onSelectFile(files[0].path);
  }, [files, loading, onSelectFile, selectedPath]);

  return (
    <div className="relative h-full overflow-auto pb-[var(--ct-safe-bottom,0px)]">
      {commit && <CommitMetaHeader commit={commit} />}
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
    </div>
  );
}

// Shared file fetcher so wide and narrow layouts agree on state.
export function useCommitFiles(api: ApiConfig, cwd: string, sha: string) {
  const [files, setFiles] = useState<GitCommitFile[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let active = true;
    const ctrl = new AbortController();
    setLoading(true);
    setError(null);
    fetchCommitFiles(api, cwd, sha, ctrl.signal)
      .then((f) => {
        if (active) setFiles(f);
      })
      .catch((e) => {
        if (active && e.name !== "AbortError") setError(String(e.message ?? e));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
      ctrl.abort();
    };
  }, [api, cwd, sha]);

  return { files, error, loading };
}

interface FileListProps {
  files: DisplayFile[];
  error: string | null;
  loading: boolean;
  selectedPath: string | null;
  onSelect: (path: string) => void;
  onContextMenu?: (e: React.MouseEvent, file: DisplayFile) => void;
  // Path currently showing the inline revert confirm (workdir only).
  confirmPath?: string | null;
  onConfirmRevert?: (path: string) => void;
  onCancelRevert?: () => void;
  showStatus?: boolean;
  touch?: boolean;
  onToggleStaged?: (file: DisplayFile) => void;
  stagingBusyPath?: string | null;
  stagingDisabled?: boolean;
}

export function FileList({
  files,
  error,
  loading,
  selectedPath,
  onSelect,
  onContextMenu,
  confirmPath,
  onConfirmRevert,
  onCancelRevert,
  showStatus,
  touch,
  onToggleStaged,
  stagingBusyPath,
  stagingDisabled,
}: FileListProps) {
  return (
    <>
      {error && <div className="p-3 text-[11px] text-[var(--ct-red)]">{error}</div>}
      {!loading && !error && files.length === 0 && (
        <div className={`p-3 ${UI_TEXT.meta}`}>No file changes.</div>
      )}
      {files.map((f) =>
        confirmPath === f.path ? (
          <RevertConfirmRow
            key={f.path}
            file={f}
            touch={touch}
            onConfirm={() => onConfirmRevert?.(f.path)}
            onCancel={() => onCancelRevert?.()}
          />
        ) : (
          <div
            key={f.path}
            role="button"
            tabIndex={0}
            onClick={() => onSelect(f.path)}
            onKeyDown={(e) => {
              if (e.key !== "Enter" && e.key !== " ") return;
              if (e.target instanceof HTMLInputElement) return;
              e.preventDefault();
              onSelect(f.path);
            }}
            onContextMenu={onContextMenu ? (e) => onContextMenu(e, f) : undefined}
            className={`w-full text-left px-3 flex items-center gap-2 text-[12px] border-b border-[color-mix(in_srgb,var(--ct-border)_35%,transparent)] ${
              touch ? "py-2.5 min-h-[40px]" : "py-1.5"
            } ${
              selectedPath === f.path
                ? "bg-[color-mix(in_srgb,var(--ct-accent)_16%,transparent)]"
                : "hover:bg-[color-mix(in_srgb,var(--ct-fg)_5%,transparent)]"
            }`}
          >
            {onToggleStaged && (
              <StagingCheckbox
                file={f}
                busy={stagingBusyPath === f.path}
                disabled={stagingDisabled === true}
                onToggle={() => onToggleStaged(f)}
              />
            )}
            {showStatus && f.status && <StatusBadge status={f.status} />}
            <FileStat added={f.added} deleted={f.deleted} />
            <span className="flex-1 min-w-0 truncate text-[var(--ct-fg)]">
              {f.old_path ? (
                <span title={`${f.old_path} → ${f.path}`}>
                  <span className="text-[var(--ct-muted)]">{basename(f.old_path)} → </span>
                  {basename(f.path)}
                </span>
              ) : (
                <span title={f.path}>{f.path}</span>
              )}
            </span>
          </div>
        ),
      )}
    </>
  );
}

export function FileListLoadingBadge({ loading }: { loading: boolean }) {
  if (!loading) return null;
  return (
    <div className="ct-git-loading-badge">
      <span className="ct-git-spinner" aria-hidden />
      <span>Loading files</span>
    </div>
  );
}

function StagingCheckbox({
  file,
  busy,
  disabled,
  onToggle,
}: {
  file: DisplayFile;
  busy: boolean;
  disabled: boolean;
  onToggle: () => void;
}) {
  const staged = file.staged === true;
  const inputDisabled = disabled || busy || file.status === "conflicted";
  return (
    <input
      type="checkbox"
      checked={staged}
      disabled={inputDisabled}
      aria-label={staged ? `Unstage ${file.path}` : `Stage ${file.path}`}
      title={staged ? "Unstage" : "Stage"}
      onClick={(e) => e.stopPropagation()}
      onChange={onToggle}
      className="ct-git-stage-checkbox"
    />
  );
}

function RevertConfirmRow({
  file,
  touch,
  onConfirm,
  onCancel,
}: {
  file: DisplayFile;
  touch?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <div
      className={`w-full px-3 flex items-center gap-2 text-[12px] border-b border-[color-mix(in_srgb,var(--ct-border)_35%,transparent)] bg-[color-mix(in_srgb,var(--ct-yellow)_12%,transparent)] ${
        touch ? "py-2.5 min-h-[40px]" : "py-1.5"
      }`}
    >
      <span className="flex-1 min-w-0 truncate text-[var(--ct-fg)]">
        Revert {basename(file.path)}?
      </span>
      <button
        type="button"
        onClick={onConfirm}
        title="Confirm revert"
        className="shrink-0 px-1.5 text-[var(--ct-green)] hover:opacity-80"
      >
        ✓
      </button>
      <button
        type="button"
        onClick={onCancel}
        title="Cancel"
        className="shrink-0 px-1.5 text-[var(--ct-red)] hover:opacity-80"
      >
        ✕
      </button>
    </div>
  );
}

const STATUS_LABEL: Record<string, string> = {
  staged: "S",
  modified: "M",
  untracked: "U",
  deleted: "D",
  renamed: "R",
  conflicted: "C",
};

function StatusBadge({ status }: { status: string }) {
  return (
    <span className={`ct-git-status-badge kind-${status}`} title={status}>
      {STATUS_LABEL[status] ?? status.slice(0, 1).toUpperCase()}
    </span>
  );
}

// Narrow-layout header shown above the file list at drill level 2.
export function CommitMetaHeader({ commit }: { commit: GitCommit }) {
  return (
    <div className="px-3 py-2 border-b border-[var(--ct-border)]">
      <div className="text-[13px] text-[var(--ct-fg)] font-medium break-words">{commit.subject}</div>
      <div className="mt-0.5 flex items-center gap-2 text-[11px] text-[var(--ct-muted-fg)]">
        <span className="truncate">{commit.author}</span>
        <span className="font-mono opacity-80">{commit.sha.slice(0, 7)}</span>
      </div>
    </div>
  );
}

function FileStat({ added, deleted }: { added: number | null; deleted: number | null }) {
  return (
    <span className="font-mono text-[10px] tabular-nums shrink-0 flex gap-1.5">
      <span className="text-[var(--ct-green)]">+{added ?? "–"}</span>
      <span className="text-[var(--ct-red)]">−{deleted ?? "–"}</span>
    </span>
  );
}

function basename(p: string): string {
  const idx = p.lastIndexOf("/");
  return idx === -1 ? p : p.slice(idx + 1);
}

interface DiffPaneProps {
  api: ApiConfig;
  cwd: string;
  sha: string;
  path: string;
  split: boolean;
}

export function DiffPane({ api, cwd, sha, path, split }: DiffPaneProps) {
  const [patch, setPatch] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const scrollRef = useRef<HTMLDivElement>(null);
  const scrollKey = `${cwd}\u0000${sha}\u0000${path}\u0000${split ? "split" : "unified"}`;

  useEffect(() => {
    let active = true;
    const ctrl = new AbortController();
    setLoading(true);
    setError(null);
    setPatch(null);
    const fetcher =
      sha === WORKDIR_SHA
        ? fetchWorkdirDiff(api, cwd, path, ctrl.signal)
        : fetchCommitDiff(api, cwd, sha, path, ctrl.signal);
    fetcher
      .then((text) => {
        if (active) setPatch(text);
      })
      .catch((e) => {
        if (active && e.name !== "AbortError") setError(String(e.message ?? e));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
      ctrl.abort();
    };
  }, [api, cwd, sha, path]);

  const truncated = patch?.includes("\n<<TRUNCATED>>") ?? false;
  const isBinary = useMemo(
    () => (patch ? /Binary files .* differ/.test(patch) : false),
    [patch],
  );
  const lang = useMemo(() => langFor(path), [path]);

  useEffect(() => {
    const node = scrollRef.current;
    return () => {
      if (node) rememberDiffScroll(scrollKey, node.scrollTop, node.scrollLeft);
    };
  }, [scrollKey]);

  useEffect(() => {
    const node = scrollRef.current;
    const pos = diffScrollPositions.get(scrollKey);
    if (!node || !pos || loading || error) return;
    const raf = window.requestAnimationFrame(() => {
      node.scrollTop = pos.top;
      node.scrollLeft = pos.left;
    });
    return () => window.cancelAnimationFrame(raf);
  }, [scrollKey, loading, error, patch]);

  return (
    <div
      ref={scrollRef}
      className="ct-git-diff h-full overflow-auto"
      onScroll={(e) => {
        rememberDiffScroll(scrollKey, e.currentTarget.scrollTop, e.currentTarget.scrollLeft);
      }}
    >
      {loading && <div className={`p-4 ${UI_TEXT.meta}`}>Loading diff…</div>}
      {error && <div className="p-4 text-[11px] text-[var(--ct-red)]">{error}</div>}
      {!loading && !error && !patch && null}
      {!loading && !error && isBinary && (
        <div className={`p-4 ${UI_TEXT.meta}`}>Binary file — no preview.</div>
      )}
      {truncated && (
        <div className="px-4 py-2 text-[11px] text-[var(--ct-yellow)] border-b border-[var(--ct-border)]">
          diff truncated
        </div>
      )}
      {!loading && !error && patch && !isBinary && (
        <DiffView
          data={{
            hunks: [patch],
            oldFile: { fileName: path, fileLang: lang },
            newFile: { fileName: path, fileLang: lang },
          }}
          diffViewMode={split ? DiffModeEnum.Split : DiffModeEnum.Unified}
          diffViewTheme="dark"
          diffViewHighlight
          diffViewWrap={false}
          key={`${sha}:${path}:${lang}`}
        />
      )}
    </div>
  );
}

function langFor(path: string): string {
  const ext = path.slice(path.lastIndexOf(".") + 1).toLowerCase();
  const map: Record<string, string> = {
    ts: "typescript",
    tsx: "tsx",
    js: "javascript",
    jsx: "jsx",
    rs: "rust",
    py: "python",
    go: "go",
    css: "css",
    json: "json",
    md: "markdown",
    sh: "bash",
    yml: "yaml",
    yaml: "yaml",
    toml: "ini",
    swift: "swift",
    html: "xml",
  };
  return map[ext] ?? "plaintext";
}
