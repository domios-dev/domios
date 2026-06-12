import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ApiConfig } from "../../lib/types";
import { ContextMenu, type ContextMenuAction } from "../Terminal/ContextMenu";
import {
  fetchWorkdirFiles,
  revertFile,
  WORKDIR_SHA,
  type GitWorkdirFile,
} from "./gitApi";
import { DiffPane, FileList, type DisplayFile } from "./CommitDetail";
import { DiffModal } from "./DiffModal";
import { useDragRatio } from "./useDragRatio";
import { UI_TEXT } from "../shared/design";

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
  openMenu: (e: React.MouseEvent, file: DisplayFile) => void;
  closeMenu: () => void;
  beginRevert: (path: string) => void;
  cancelRevert: () => void;
  doRevert: (path: string) => void;
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
      revertFile(api, cwd, path)
        .then(() => {
          setNonce((n) => n + 1);
          onReverted();
        })
        .catch(() => {
          setNonce((n) => n + 1);
        });
    },
    [api, cwd, onReverted],
  );

  return {
    files,
    error,
    loading,
    menu,
    confirmPath,
    openMenu,
    closeMenu,
    beginRevert,
    cancelRevert,
    doRevert,
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

// Wide layout: file list stacked over diff with draggable h-divider.
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
  const { ratio, onPointerDown, reset } = useDragRatio(boxRef, {
    storageKey: "ct-git-hsplit",
    axis: "y",
    minPx: 120,
    defaultRatio: 0.35,
  });

  const effectivePath = selectedPath ?? wd.files[0]?.path ?? null;

  return (
    <div ref={boxRef} className="flex flex-col h-full min-h-0">
      <div className="overflow-auto min-h-0" style={{ flex: `0 0 ${ratio * 100}%` }}>
        <WorkdirHeader />
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
        />
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
          <DiffPane api={api} cwd={cwd} sha={WORKDIR_SHA} path={effectivePath} split={split} />
        ) : (
          <div className={`p-4 ${UI_TEXT.meta}`}>Select a file to view its diff.</div>
        )}
      </div>
      <WorkdirMenu state={wd} />
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
  const selectedFile = useMemo(
    () => wd.files.find((f) => f.path === selectedPath) ?? null,
    [wd.files, selectedPath],
  );

  return (
    <div className="absolute inset-0 overflow-auto pb-[var(--ct-safe-bottom,0px)]">
      <WorkdirHeader />
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
        touch={touch}
      />
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
    </div>
  );
}

function WorkdirHeader() {
  return (
    <div className="px-3 py-2 border-b border-[var(--ct-border)]">
      <div className="text-[13px] text-[var(--ct-yellow)] font-medium">Uncommitted changes</div>
      <div className="mt-0.5 text-[11px] text-[var(--ct-muted-fg)]">Working tree vs HEAD</div>
    </div>
  );
}
