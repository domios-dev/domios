import { useEffect, useMemo, useState } from "react";
import { DiffView, DiffModeEnum } from "@git-diff-view/react";
import "@git-diff-view/react/styles/diff-view.css";
import type { ApiConfig } from "../../lib/types";
import {
  fetchCommitDiff,
  fetchCommitFiles,
  type GitCommit,
  type GitCommitFile,
} from "./gitApi";
import { UI_TEXT } from "../shared/design";

interface CommitDetailProps {
  api: ApiConfig;
  cwd: string;
  sha: string;
  split: boolean;
}

// Wide-layout detail: file list + diff side by side. Owns file fetching.
export function CommitDetail({ api, cwd, sha, split }: CommitDetailProps) {
  const { files, error, loading } = useCommitFiles(api, cwd, sha);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);

  useEffect(() => {
    setSelectedPath(files[0]?.path ?? null);
  }, [files]);

  return (
    <div className="flex h-full min-h-0">
      <div className="w-64 shrink-0 overflow-auto border-r border-[var(--ct-border)]">
        <FileList
          files={files}
          error={error}
          loading={loading}
          selectedPath={selectedPath}
          onSelect={setSelectedPath}
        />
      </div>
      <div className="flex-1 min-w-0 overflow-auto">
        {selectedPath ? (
          <DiffPane api={api} cwd={cwd} sha={sha} path={selectedPath} split={split} />
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
  files: GitCommitFile[];
  error: string | null;
  loading: boolean;
  selectedPath: string | null;
  onSelect: (path: string) => void;
  touch?: boolean;
}

export function FileList({ files, error, loading, selectedPath, onSelect, touch }: FileListProps) {
  return (
    <>
      {loading && <div className={`p-3 ${UI_TEXT.meta}`}>Loading files…</div>}
      {error && <div className="p-3 text-[11px] text-[var(--ct-red)]">{error}</div>}
      {!loading && !error && files.length === 0 && (
        <div className={`p-3 ${UI_TEXT.meta}`}>No file changes.</div>
      )}
      {files.map((f) => (
        <button
          key={f.path}
          type="button"
          onClick={() => onSelect(f.path)}
          className={`w-full text-left px-3 flex items-center gap-2 text-[12px] border-b border-[color-mix(in_srgb,var(--ct-border)_35%,transparent)] ${
            touch ? "py-2.5 min-h-[40px]" : "py-1.5"
          } ${
            selectedPath === f.path
              ? "bg-[color-mix(in_srgb,var(--ct-accent)_16%,transparent)]"
              : "hover:bg-[color-mix(in_srgb,var(--ct-fg)_5%,transparent)]"
          }`}
        >
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
      ))}
    </>
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

function FileStat({ added, deleted }: { added: number; deleted: number }) {
  return (
    <span className="font-mono text-[10px] tabular-nums shrink-0 flex gap-1.5">
      <span className="text-[var(--ct-green)]">+{added}</span>
      <span className="text-[var(--ct-red)]">−{deleted}</span>
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
    fetchCommitDiff(api, cwd, sha, path, ctrl.signal)
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
