import { useEffect, useMemo, useRef, useState } from "react";
import { DiffView, DiffModeEnum } from "@git-diff-view/react";
import "@git-diff-view/react/styles/diff-view.css";
import type { ApiConfig } from "../../lib/types";
import {
  fetchCommitDiff,
  fetchCommitFiles,
  fetchWorkdirDiff,
  type GitCommit,
  type GitCommitFile,
  WORKDIR_SHA,
} from "./gitApi";
import { useDragRatio } from "./useDragRatio";
import { ContextMenu } from "../Terminal/ContextMenu";
import { UI_TEXT } from "../shared/design";

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
}

interface CommitDetailProps {
  api: ApiConfig;
  cwd: string;
  sha: string;
  split: boolean;
  selectedPath: string | null;
  onSelectFile: (path: string) => void;
}

// Wide-layout detail: file list stacked over diff, draggable h-divider between.
// Selection is controlled by the panel so it survives layout-mode crossings.
export function CommitDetail({ api, cwd, sha, split, selectedPath, onSelectFile }: CommitDetailProps) {
  const { files, error, loading } = useCommitFiles(api, cwd, sha);
  const copyMenu = useCopyPathMenu();
  const boxRef = useRef<HTMLDivElement>(null);
  const { ratio, onPointerDown, reset } = useDragRatio(boxRef, {
    storageKey: "ct-git-hsplit",
    axis: "y",
    minPx: 120,
    defaultRatio: 0.35,
  });

  // Wide always shows a diff: fall back to the first file for display only,
  // without writing panel state (so leaving wide doesn't auto-open the modal).
  const effectivePath = selectedPath ?? files[0]?.path ?? null;

  return (
    <div ref={boxRef} className="flex flex-col h-full min-h-0">
      <div className="overflow-auto min-h-0" style={{ flex: `0 0 ${ratio * 100}%` }}>
        <FileList
          files={files}
          error={error}
          loading={loading}
          selectedPath={effectivePath}
          onSelect={onSelectFile}
          onContextMenu={copyMenu.open}
        />
        {copyMenu.node}
      </div>
      <div
        className="ct-git-divider ct-git-divider--h"
        onPointerDown={onPointerDown}
        onDoubleClick={reset}
        role="separator"
        aria-orientation="horizontal"
      />
      <div className="flex-1 min-h-0 overflow-auto">
        {effectivePath ? (
          <DiffPane api={api} cwd={cwd} sha={sha} path={effectivePath} split={split} />
        ) : (
          <div className={`p-4 ${UI_TEXT.meta}`}>Select a file to view its diff.</div>
        )}
      </div>
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
}: FileListProps) {
  return (
    <>
      {loading && <div className={`p-3 ${UI_TEXT.meta}`}>Loading files…</div>}
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
          <button
            key={f.path}
            type="button"
            onClick={() => onSelect(f.path)}
            onContextMenu={onContextMenu ? (e) => onContextMenu(e, f) : undefined}
            className={`w-full text-left px-3 flex items-center gap-2 text-[12px] border-b border-[color-mix(in_srgb,var(--ct-border)_35%,transparent)] ${
              touch ? "py-2.5 min-h-[40px]" : "py-1.5"
            } ${
              selectedPath === f.path
                ? "bg-[color-mix(in_srgb,var(--ct-accent)_16%,transparent)]"
                : "hover:bg-[color-mix(in_srgb,var(--ct-fg)_5%,transparent)]"
            }`}
          >
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
          </button>
        ),
      )}
    </>
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

  if (loading) return <div className={`p-4 ${UI_TEXT.meta}`}>Loading diff…</div>;
  if (error) return <div className="p-4 text-[11px] text-[var(--ct-red)]">{error}</div>;
  if (!patch) return null;
  if (isBinary) {
    return <div className={`p-4 ${UI_TEXT.meta}`}>Binary file — no preview.</div>;
  }

  return (
    <div className="ct-git-diff">
      {truncated && (
        <div className="px-4 py-2 text-[11px] text-[var(--ct-yellow)] border-b border-[var(--ct-border)]">
          diff truncated
        </div>
      )}
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
