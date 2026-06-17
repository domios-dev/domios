import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { GitCommit, Upload, X } from "lucide-react";
import type { ApiConfig } from "./types";
import { ContextMenu, type ContextMenuAction } from "./ContextMenu";
import {
  commitRepo,
  fetchWorkdirFiles,
  pushRepo,
  revertFile,
  stageFile,
  stageFiles,
  unstageFile,
  unstageFiles,
  WORKDIR_SHA,
  type GitWorkdirFile,
} from "./gitApi";
import { DiffPane, FileList, FileListLoadingBadge, type DisplayFile } from "./CommitDetail";
import { DiffModal } from "./DiffModal";
import { useDragRatio } from "./useDragRatio";
import { UI_BUTTON, UI_TEXT } from "./design";

type WorkdirBusyAction = "commit" | "push" | "stage" | null;

export function useWorkdirFiles(api: ApiConfig, cwd: string, nonce: number) {
  const [files, setFiles] = useState<GitWorkdirFile[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let active = true;
    const ctrl = new AbortController();
    setLoading(true);
    setError(null);
    fetchWorkdirFiles(api, cwd, ctrl.signal)
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
  }, [api, cwd, nonce]);

  return { files, error, loading };
}

interface WorkdirState {
  files: GitWorkdirFile[];
  error: string | null;
  loading: boolean;
  menu: { x: number; y: number; file: DisplayFile } | null;
  confirmPath: string | null;
  stagedCount: number;
  stageableCount: number;
  busyPath: string | null;
  busyAction: WorkdirBusyAction;
  actionError: string | null;
  openMenu: (e: React.MouseEvent, file: DisplayFile) => void;
  closeMenu: () => void;
  beginRevert: (path: string) => void;
  cancelRevert: () => void;
  doRevert: (path: string) => void;
  toggleStaged: (file: DisplayFile) => void;
  toggleAllStaged: () => void;
  commit: (message: string) => Promise<boolean>;
  push: () => void;
}

// Shared workdir state: file fetch, context menu, inline revert confirm.
function useWorkdir(
  api: ApiConfig,
  cwd: string,
  onReverted: () => void,
): WorkdirState {
  const [nonce, setNonce] = useState(0);
  const { files, error, loading } = useWorkdirFiles(api, cwd, nonce);
  const [menu, setMenu] = useState<WorkdirState["menu"]>(null);
  const [confirmPath, setConfirmPath] = useState<string | null>(null);
  const [busyPath, setBusyPath] = useState<string | null>(null);
  const [busyAction, setBusyAction] = useState<WorkdirBusyAction>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const stageableFiles = useMemo(() => files.filter((f) => f.status !== "conflicted"), [files]);
  const stagedCount = useMemo(() => stageableFiles.filter((f) => f.staged).length, [stageableFiles]);
  const stageableCount = stageableFiles.length;
  const refresh = useCallback(() => setNonce((n) => n + 1), []);

  const openMenu = useCallback((e: React.MouseEvent, file: DisplayFile) => {
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY, file });
  }, []);
  const closeMenu = useCallback(() => setMenu(null), []);
  const beginRevert = useCallback((path: string) => setConfirmPath(path), []);
  const cancelRevert = useCallback(() => setConfirmPath(null), []);

  const doRevert = useCallback(
    (path: string) => {
      setConfirmPath(null);
      setActionError(null);
      revertFile(api, cwd, path)
        .then(() => {
          refresh();
          onReverted();
        })
        .catch((e) => {
          setActionError(String((e as Error).message ?? e));
          refresh();
        });
    },
    [api, cwd, onReverted, refresh],
  );

  const toggleStaged = useCallback(
    (file: DisplayFile) => {
      if (busyAction !== null || busyPath !== null) return;
      if (file.status === "conflicted") return;
      setBusyPath(file.path);
      setActionError(null);
      const op = file.staged === true ? unstageFile(api, cwd, file.path) : stageFile(api, cwd, file.path);
      op.then(() => {
        refresh();
        onReverted();
      })
        .catch((e) => {
          setActionError(String((e as Error).message ?? e));
          refresh();
        })
        .finally(() => setBusyPath((cur) => (cur === file.path ? null : cur)));
    },
    [api, busyAction, busyPath, cwd, onReverted, refresh],
  );

  const toggleAllStaged = useCallback(() => {
    if (busyAction !== null || busyPath !== null) return;
    if (stageableFiles.length === 0) return;
    setBusyAction("stage");
    setActionError(null);
    const allStaged = stageableFiles.every((file) => file.staged);
    const targets = allStaged
      ? stageableFiles.filter((file) => file.staged)
      : stageableFiles.filter((file) => !file.staged);
    const op = allStaged ? unstageFiles : stageFiles;
    void (async () => {
      try {
        await op(api, cwd, targets.map((file) => file.path));
        refresh();
        onReverted();
      } catch (e) {
        setActionError(String((e as Error).message ?? e));
        refresh();
      } finally {
        setBusyAction(null);
      }
    })();
  }, [api, busyAction, busyPath, cwd, onReverted, refresh, stageableFiles]);

  const commit = useCallback(
    async (message: string) => {
      setBusyAction("commit");
      setActionError(null);
      try {
        await commitRepo(api, cwd, message);
        refresh();
        onReverted();
        return true;
      } catch (e) {
        setActionError(String((e as Error).message ?? e));
        refresh();
        return false;
      } finally {
        setBusyAction(null);
      }
    },
    [api, cwd, onReverted, refresh],
  );

  const push = useCallback(() => {
    setBusyAction("push");
    setActionError(null);
    pushRepo(api, cwd)
      .then(() => {
        refresh();
        onReverted();
      })
      .catch((e) => setActionError(String((e as Error).message ?? e)))
      .finally(() => setBusyAction(null));
  }, [api, cwd, onReverted, refresh]);

  return {
    files,
    error,
    loading,
    menu,
    confirmPath,
    stagedCount,
    stageableCount,
    busyPath,
    busyAction,
    actionError,
    openMenu,
    closeMenu,
    beginRevert,
    cancelRevert,
    doRevert,
    toggleStaged,
    toggleAllStaged,
    commit,
    push,
  };
}

function copyPath(p: string) {
  void navigator.clipboard?.writeText(p);
}

function WorkdirMenu({ state }: { state: WorkdirState }) {
  if (!state.menu) return null;
  const file = state.menu.file;
  const actions: ContextMenuAction[] = [
    { label: "Revert file…", onClick: () => state.beginRevert(file.path) },
    { label: "Copy path", onClick: () => copyPath(file.path) },
  ];
  return <ContextMenu x={state.menu.x} y={state.menu.y} actions={actions} onClose={state.closeMenu} />;
}

interface WorkdirDetailProps {
  api: ApiConfig;
  cwd: string;
  selectedPath: string | null;
  split: boolean;
  onSelectFile: (path: string) => void;
  onReverted: () => void;
}

// Wide layout: file list on the left, diff preview on the right.
export function WorkdirDetail({
  api,
  cwd,
  selectedPath,
  split,
  onSelectFile,
  onReverted,
}: WorkdirDetailProps) {
  const wd = useWorkdir(api, cwd, onReverted);
  const boxRef = useRef<HTMLDivElement>(null);
  const [commitOpen, setCommitOpen] = useState(false);
  const { ratio, onPointerDown, reset } = useDragRatio(boxRef, {
    storageKey: "ct-git-workdir-files-split",
    axis: "x",
    minPx: 220,
    defaultRatio: 0.34,
  });

  const effectivePath = selectedPath ?? wd.files[0]?.path ?? null;

  return (
    <div ref={boxRef} className="flex h-full min-h-0">
      <div className="relative overflow-auto min-w-0" style={{ flex: `0 0 ${ratio * 100}%` }}>
        <WorkdirHeader
          fileCount={wd.files.length}
          stagedCount={wd.stagedCount}
          stageableCount={wd.stageableCount}
          busyAction={wd.busyAction}
          actionError={wd.actionError}
          onToggleAllStaged={wd.toggleAllStaged}
          onCommitClick={() => setCommitOpen(true)}
          onPush={wd.push}
        />
        <FileList
          files={wd.files}
          error={wd.error}
          loading={wd.loading}
          selectedPath={effectivePath}
          onSelect={onSelectFile}
          onContextMenu={wd.openMenu}
          confirmPath={wd.confirmPath}
          onConfirmRevert={wd.doRevert}
          onCancelRevert={wd.cancelRevert}
          showStatus
          onToggleStaged={wd.toggleStaged}
          stagingBusyPath={wd.busyPath}
          stagingDisabled={wd.busyAction !== null || wd.busyPath !== null}
        />
        <FileListLoadingBadge loading={wd.loading} />
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
          <DiffPane api={api} cwd={cwd} sha={WORKDIR_SHA} path={effectivePath} split={split} />
        ) : (
          <div className={`p-4 ${UI_TEXT.meta}`}>Select a file to view its diff.</div>
        )}
      </div>
      <WorkdirMenu state={wd} />
      <CommitDialog
        open={commitOpen}
        stagedCount={wd.stagedCount}
        busy={wd.busyAction === "commit"}
        error={wd.actionError}
        onClose={() => setCommitOpen(false)}
        onSubmit={wd.commit}
      />
    </div>
  );
}

interface WorkdirFilesPaneProps {
  api: ApiConfig;
  cwd: string;
  selectedPath: string | null;
  onSelectFile: (path: string) => void;
  onReverted: () => void;
  touch?: boolean;
}

export function WorkdirFilesPane({
  api,
  cwd,
  selectedPath,
  onSelectFile,
  onReverted,
  touch,
}: WorkdirFilesPaneProps) {
  const wd = useWorkdir(api, cwd, onReverted);
  const [commitOpen, setCommitOpen] = useState(false);

  useEffect(() => {
    if (wd.loading || wd.files.length === 0 || wd.files.some((f) => f.path === selectedPath)) return;
    onSelectFile(wd.files[0].path);
  }, [onSelectFile, selectedPath, wd.files, wd.loading]);

  return (
    <div className="relative h-full overflow-auto pb-[var(--ct-safe-bottom,0px)]">
      <WorkdirHeader
        fileCount={wd.files.length}
        stagedCount={wd.stagedCount}
        stageableCount={wd.stageableCount}
        busyAction={wd.busyAction}
        actionError={wd.actionError}
        onToggleAllStaged={wd.toggleAllStaged}
        onCommitClick={() => setCommitOpen(true)}
        onPush={wd.push}
      />
      <FileList
        files={wd.files}
        error={wd.error}
        loading={wd.loading}
        selectedPath={selectedPath}
        onSelect={onSelectFile}
        onContextMenu={wd.openMenu}
        confirmPath={wd.confirmPath}
        onConfirmRevert={wd.doRevert}
        onCancelRevert={wd.cancelRevert}
        showStatus
        onToggleStaged={wd.toggleStaged}
        stagingBusyPath={wd.busyPath}
        stagingDisabled={wd.busyAction !== null || wd.busyPath !== null}
        touch={touch}
      />
      <FileListLoadingBadge loading={wd.loading} />
      <WorkdirMenu state={wd} />
      <CommitDialog
        open={commitOpen}
        stagedCount={wd.stagedCount}
        busy={wd.busyAction === "commit"}
        error={wd.actionError}
        onClose={() => setCommitOpen(false)}
        onSubmit={wd.commit}
      />
    </div>
  );
}

interface WorkdirFilesWithDiffProps {
  api: ApiConfig;
  cwd: string;
  selectedPath: string | null;
  split: boolean;
  onToggleSplit: () => void;
  onSelectFile: (path: string) => void;
  onCloseDiff: () => void;
  onReverted: () => void;
  fullscreen?: boolean;
  touch?: boolean;
}

// Medium/narrow layout: file list with DiffModal overlay (mirrors FilesWithDiff).
export function WorkdirFilesWithDiff({
  api,
  cwd,
  selectedPath,
  split,
  onToggleSplit,
  onSelectFile,
  onCloseDiff,
  onReverted,
  fullscreen,
  touch,
}: WorkdirFilesWithDiffProps) {
  const wd = useWorkdir(api, cwd, onReverted);
  const [commitOpen, setCommitOpen] = useState(false);
  const selectedFile = useMemo(
    () => wd.files.find((f) => f.path === selectedPath) ?? null,
    [wd.files, selectedPath],
  );

  return (
    <div className="absolute inset-0 overflow-auto pb-[var(--ct-safe-bottom,0px)]">
      <WorkdirHeader
        fileCount={wd.files.length}
        stagedCount={wd.stagedCount}
        stageableCount={wd.stageableCount}
        busyAction={wd.busyAction}
        actionError={wd.actionError}
        onToggleAllStaged={wd.toggleAllStaged}
        onCommitClick={() => setCommitOpen(true)}
        onPush={wd.push}
      />
      <FileList
        files={wd.files}
        error={wd.error}
        loading={wd.loading}
        selectedPath={selectedPath}
        onSelect={onSelectFile}
        onContextMenu={wd.openMenu}
        confirmPath={wd.confirmPath}
        onConfirmRevert={wd.doRevert}
        onCancelRevert={wd.cancelRevert}
        showStatus
        onToggleStaged={wd.toggleStaged}
        stagingBusyPath={wd.busyPath}
        stagingDisabled={wd.busyAction !== null || wd.busyPath !== null}
        touch={touch}
      />
      <FileListLoadingBadge loading={wd.loading} />
      {selectedFile && (
        <DiffModal
          api={api}
          cwd={cwd}
          sha={WORKDIR_SHA}
          file={selectedFile}
          split={split}
          onToggleSplit={onToggleSplit}
          onClose={onCloseDiff}
          fullscreen={fullscreen}
        />
      )}
      <WorkdirMenu state={wd} />
      <CommitDialog
        open={commitOpen}
        stagedCount={wd.stagedCount}
        busy={wd.busyAction === "commit"}
        error={wd.actionError}
        onClose={() => setCommitOpen(false)}
        onSubmit={wd.commit}
      />
    </div>
  );
}

function WorkdirHeader({
  fileCount,
  stagedCount,
  stageableCount,
  busyAction,
  actionError,
  onToggleAllStaged,
  onCommitClick,
  onPush,
}: {
  fileCount: number;
  stagedCount: number;
  stageableCount: number;
  busyAction: WorkdirBusyAction;
  actionError: string | null;
  onToggleAllStaged: () => void;
  onCommitClick: () => void;
  onPush: () => void;
}) {
  const allRef = useRef<HTMLInputElement>(null);
  const allStaged = stageableCount > 0 && stagedCount === stageableCount;
  const partiallyStaged = stagedCount > 0 && stagedCount < stageableCount;

  useEffect(() => {
    if (allRef.current) allRef.current.indeterminate = partiallyStaged;
  }, [partiallyStaged]);

  return (
    <div className="px-3 py-2 border-b border-[var(--ct-border)]">
      <div className="flex items-center gap-2 min-w-0">
        <input
          ref={allRef}
          type="checkbox"
          checked={allStaged}
          disabled={stageableCount === 0 || busyAction !== null}
          aria-label={allStaged ? "Unstage all files" : "Stage all files"}
          title={allStaged ? "Unstage all" : "Stage all"}
          onChange={onToggleAllStaged}
          className="ct-git-stage-checkbox"
        />
        <div className="flex-1 min-w-0">
          <div className="text-[13px] text-[var(--ct-yellow)] font-medium">Uncommitted changes</div>
          <div className="mt-0.5 flex items-center gap-1.5 text-[11px] text-[var(--ct-muted-fg)]">
            {busyAction === "stage" && <span className="ct-git-spinner" aria-hidden />}
            <span>
              {busyAction === "stage" ? "Updating index" : `${stagedCount}/${stageableCount} staged`}
              {fileCount !== stageableCount ? ` · ${fileCount - stageableCount} blocked` : ""}
            </span>
          </div>
        </div>
        <button
          type="button"
          onClick={onCommitClick}
          disabled={stagedCount === 0 || busyAction !== null}
          title="Commit staged files"
          className={`${UI_BUTTON.ghost} ${UI_BUTTON.sm} shrink-0 flex items-center gap-1 !px-2`}
        >
          <GitCommit size={14} />
          <span className="text-[12px]">Commit</span>
        </button>
        <button
          type="button"
          onClick={onPush}
          disabled={busyAction !== null}
          title="Push"
          className={`${UI_BUTTON.ghost} ${UI_BUTTON.sm} shrink-0 flex items-center gap-1 !px-2`}
        >
          <Upload size={14} />
          <span className="text-[12px]">{busyAction === "push" ? "Pushing" : "Push"}</span>
        </button>
      </div>
      {actionError && (
        <div className="mt-2 text-[11px] text-[var(--ct-red)] break-words">{actionError}</div>
      )}
    </div>
  );
}

function CommitDialog({
  open,
  stagedCount,
  busy,
  error,
  onClose,
  onSubmit,
}: {
  open: boolean;
  stagedCount: number;
  busy: boolean;
  error: string | null;
  onClose: () => void;
  onSubmit: (message: string) => Promise<boolean>;
}) {
  const [message, setMessage] = useState("");
  const inputRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    if (!open) return;
    setMessage("");
    const raf = window.requestAnimationFrame(() => inputRef.current?.focus());
    return () => window.cancelAnimationFrame(raf);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopPropagation();
      onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [open, onClose]);

  if (!open) return null;

  const trimmed = message.trim();
  const node = (
    <div className="ct-git-commit-overlay" onPointerDown={onClose}>
      <form
        className="ct-git-commit-dialog"
        onPointerDown={(e) => e.stopPropagation()}
        onSubmit={(e) => {
          e.preventDefault();
          if (!trimmed || busy) return;
          void onSubmit(trimmed).then((ok) => {
            if (ok) onClose();
          });
        }}
      >
        <div className="ct-git-commit-head">
          <div className="min-w-0">
            <div className="text-[13px] font-medium text-[var(--ct-fg)]">Commit</div>
            <div className="mt-0.5 text-[11px] text-[var(--ct-muted-fg)]">
              {stagedCount} staged {stagedCount === 1 ? "file" : "files"}
            </div>
          </div>
          <button
            type="button"
            onClick={onClose}
            title="Close"
            className={`${UI_BUTTON.icon} ${UI_BUTTON.sm} shrink-0`}
          >
            <X size={14} />
          </button>
        </div>
        <textarea
          ref={inputRef}
          value={message}
          onChange={(e) => setMessage(e.target.value)}
          className="ct-git-commit-textarea"
          placeholder="Commit message"
          rows={5}
        />
        {error && <div className="text-[11px] text-[var(--ct-red)] break-words">{error}</div>}
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            disabled={busy}
            className={`${UI_BUTTON.ghost} ${UI_BUTTON.sm}`}
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={!trimmed || stagedCount === 0 || busy}
            className={`${UI_BUTTON.primary} ${UI_BUTTON.sm} flex items-center gap-1`}
          >
            <GitCommit size={14} />
            <span>{busy ? "Committing" : "Commit"}</span>
          </button>
        </div>
      </form>
    </div>
  );

  return createPortal(node, document.body);
}
