import { useCallback, useEffect, useMemo, useState } from "react";
import { ChevronLeft, Folder, FolderPlus, Home, Loader2, RefreshCw, X } from "lucide-react";
import { invoke } from "../../lib/bridge";

interface DirEntry {
  name: string;
  path: string;
}

interface DirListing {
  path: string;
  parent?: string | null;
  home?: string | null;
  roots: DirEntry[];
  entries: DirEntry[];
}

interface RemoteFolderPickerProps {
  title: string;
  initialPath?: string | null;
  onCancel: () => void;
  onSelect: (path: string) => void;
}

function basename(path: string): string {
  const clean = path.replace(/[\\/]+$/, "");
  return clean.split(/[\\/]/).pop() || clean || path;
}

export function RemoteFolderPicker({
  title,
  initialPath,
  onCancel,
  onSelect,
}: RemoteFolderPickerProps) {
  const [path, setPath] = useState(initialPath || "");
  const [manualPath, setManualPath] = useState(initialPath || "");
  const [listing, setListing] = useState<DirListing | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showHidden, setShowHidden] = useState(false);
  const [newFolderName, setNewFolderName] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  const load = useCallback(async (nextPath: string) => {
    setLoading(true);
    setError(null);
    try {
      const result = await invoke<DirListing>("fs_list_dir", {
        path: nextPath,
        showHidden,
      });
      setListing(result);
      setPath(result.path);
      setManualPath(result.path);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [showHidden]);

  useEffect(() => {
    void load(initialPath || "");
  }, [initialPath, load]);

  const createFolder = useCallback(async () => {
    const name = newFolderName?.trim();
    if (!name || !path) return;
    setCreating(true);
    setError(null);
    try {
      const created = await invoke<{ path: string }>("fs_mkdir", {
        parent: path,
        name,
      });
      setNewFolderName(null);
      // Jump straight into the new folder — that's what you made it for.
      await load(created.path);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setCreating(false);
    }
  }, [newFolderName, path, load]);

  const rootItems = useMemo(() => {
    const items = listing?.roots ?? [];
    if (listing?.home && !items.some((item) => item.path === listing.home)) {
      return [{ name: "Home", path: listing.home }, ...items];
    }
    return items;
  }, [listing]);

  return (
    <>
      <div className="fixed inset-0 z-50 bg-black/45" onClick={onCancel} />
      <div
        className="fixed z-50 left-1/2 top-1/2 flex w-[min(92vw,520px)] max-h-[82vh] -translate-x-1/2 -translate-y-1/2 flex-col overflow-hidden rounded-lg border shadow-2xl"
        style={{
          background: "var(--ct-surface, #161b22)",
          borderColor: "var(--ct-border, #30363d)",
          color: "var(--ct-fg, #c9d1d9)",
        }}
      >
        <div className="flex items-center gap-2 border-b border-[var(--ct-border)] px-3 py-2">
          <Folder size={16} className="shrink-0 text-[var(--ct-accent,#58a6ff)]" />
          <div className="min-w-0 flex-1">
            <div className="truncate text-sm font-medium">{title}</div>
            <div className="truncate text-[11px] text-[var(--ct-muted-fg)]">{path || "Loading..."}</div>
          </div>
          <button
            type="button"
            onClick={onCancel}
            className="flex h-9 w-9 items-center justify-center rounded hover:bg-[var(--ct-hover)]"
            title="Close"
          >
            <X size={16} />
          </button>
        </div>

        <div className="flex flex-wrap items-center gap-1 border-b border-[var(--ct-border)] px-3 py-2">
          <button
            type="button"
            disabled={!listing?.parent || loading}
            onClick={() => listing?.parent && load(listing.parent)}
            className="flex h-8 items-center gap-1 rounded px-2 text-xs disabled:opacity-40 hover:bg-[var(--ct-hover)]"
          >
            <ChevronLeft size={14} />
            Up
          </button>
          {listing?.home && (
            <button
              type="button"
              disabled={loading}
              onClick={() => listing.home && load(listing.home)}
              className="flex h-8 items-center gap-1 rounded px-2 text-xs hover:bg-[var(--ct-hover)]"
            >
              <Home size={14} />
              Home
            </button>
          )}
          <button
            type="button"
            disabled={loading}
            onClick={() => load(path)}
            className="flex h-8 items-center gap-1 rounded px-2 text-xs hover:bg-[var(--ct-hover)]"
          >
            <RefreshCw size={13} />
            Refresh
          </button>
          <button
            type="button"
            data-testid="folder-picker-new"
            disabled={loading || !path}
            onClick={() => setNewFolderName((v) => (v === null ? "" : null))}
            className="flex h-8 items-center gap-1 rounded px-2 text-xs hover:bg-[var(--ct-hover)] disabled:opacity-40"
          >
            <FolderPlus size={13} />
            New folder
          </button>
          <label className="ml-auto flex h-8 items-center gap-1.5 text-xs text-[var(--ct-muted-fg)]">
            <input
              type="checkbox"
              checked={showHidden}
              onChange={(e) => setShowHidden(e.target.checked)}
              className="accent-[var(--ct-accent,#58a6ff)]"
            />
            Hidden
          </label>
        </div>

        {newFolderName !== null && (
          <form
            className="flex gap-2 border-b border-[var(--ct-border)] px-3 py-2"
            onSubmit={(e) => {
              e.preventDefault();
              void createFolder();
            }}
          >
            <input
              autoFocus
              data-testid="folder-picker-new-name"
              value={newFolderName}
              onChange={(e) => setNewFolderName(e.target.value)}
              className="min-w-0 flex-1 rounded border border-[var(--ct-border)] bg-[var(--ct-bg)] px-2 py-1.5 text-xs outline-none"
              placeholder="New folder name"
            />
            <button
              type="submit"
              data-testid="folder-picker-new-create"
              disabled={creating || !newFolderName.trim()}
              className="rounded border border-[var(--ct-accent,#58a6ff)] px-3 text-xs text-[var(--ct-accent,#58a6ff)] hover:bg-[var(--ct-hover)] disabled:opacity-40"
            >
              {creating ? "Creating…" : "Create"}
            </button>
          </form>
        )}

        <form
          className="flex gap-2 border-b border-[var(--ct-border)] px-3 py-2"
          onSubmit={(e) => {
            e.preventDefault();
            void load(manualPath);
          }}
        >
          <input
            value={manualPath}
            onChange={(e) => setManualPath(e.target.value)}
            className="min-w-0 flex-1 rounded border border-[var(--ct-border)] bg-[var(--ct-bg)] px-2 py-1.5 text-xs outline-none"
            placeholder="Path on target machine"
          />
          <button
            type="submit"
            disabled={loading || !manualPath.trim()}
            className="rounded border border-[var(--ct-border)] px-3 text-xs hover:bg-[var(--ct-hover)] disabled:opacity-40"
          >
            Go
          </button>
        </form>

        {rootItems.length > 0 && (
          <div className="flex gap-1 overflow-x-auto border-b border-[var(--ct-border)] px-3 py-2">
            {rootItems.map((root) => (
              <button
                key={root.path}
                type="button"
                onClick={() => load(root.path)}
                className="shrink-0 rounded border border-[var(--ct-border)] px-2 py-1 text-xs hover:bg-[var(--ct-hover)]"
                title={root.path}
              >
                {root.name}
              </button>
            ))}
          </div>
        )}

        <div className="min-h-0 flex-1 overflow-y-auto p-2">
          {loading && (
            <div className="flex items-center justify-center gap-2 py-8 text-sm text-[var(--ct-muted-fg)]">
              <Loader2 size={16} className="animate-spin" />
              Loading folders
            </div>
          )}
          {!loading && error && (
            <div className="rounded border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs text-red-200">
              {error}
            </div>
          )}
          {!loading && !error && listing?.entries.length === 0 && (
            <div className="py-8 text-center text-sm text-[var(--ct-muted-fg)]">No folders here.</div>
          )}
          {!loading && !error && listing?.entries.map((entry) => (
            <button
              key={entry.path}
              type="button"
              onClick={() => load(entry.path)}
              className="flex min-h-11 w-full items-center gap-2 rounded px-2 text-left hover:bg-[var(--ct-hover)]"
            >
              <Folder size={16} className="shrink-0 text-[var(--ct-muted-fg)]" />
              <span className="min-w-0 flex-1 truncate text-sm">{entry.name || basename(entry.path)}</span>
            </button>
          ))}
        </div>

        <div className="flex items-center justify-between gap-2 border-t border-[var(--ct-border)] px-3 py-2">
          <div className="min-w-0 truncate text-[11px] text-[var(--ct-muted-fg)]" title={path}>
            {path}
          </div>
          <button
            type="button"
            disabled={!path || loading}
            onClick={() => onSelect(path)}
            className="h-9 shrink-0 rounded bg-[var(--ct-accent,#58a6ff)] px-3 text-sm font-medium text-white disabled:opacity-40"
          >
            Select Folder
          </button>
        </div>
      </div>
    </>
  );
}
