import { useEffect, useMemo, useState } from "react";
import type { ApiConfig } from "../../lib/types";
import { authHeaders, baseUrl } from "./api";
import { buildMessage } from "./composer";
import { PanePicker } from "./PanePicker";
import type { DispatchablePane } from "./hooks/useDispatchablePanes";
import type { ProjectMapEdge, ProjectMapNode } from "./types";

interface Props {
  api: ApiConfig;
  selectedIds: number[];
  nodesById: Map<number, ProjectMapNode>;
  edges: ProjectMapEdge[];
  focusId: number | null;
  panes: DispatchablePane[];
  preferredPaneId: string | null;
  onPaneChange: (paneId: string) => void;
  onClear: () => void;
  onSent: () => void;
  initialMessage?: string;
}

export function ActionBar({
  api,
  selectedIds,
  nodesById,
  edges,
  focusId,
  panes,
  preferredPaneId,
  onPaneChange,
  onClear,
  onSent,
  initialMessage,
}: Props) {
  const [prompt, setPrompt] = useState(initialMessage ?? "");
  const [showList, setShowList] = useState(false);
  const [busy, setBusy] = useState(false);
  const [toast, setToast] = useState<string | null>(null);

  useEffect(() => setPrompt(initialMessage ?? ""), [initialMessage]);

  const targetPaneId = useMemo(() => {
    if (preferredPaneId && panes.some((p) => p.pane_id === preferredPaneId)) {
      return preferredPaneId;
    }
    return panes[0]?.pane_id ?? null;
  }, [panes, preferredPaneId]);

  const composed = useMemo(
    () =>
      buildMessage({ prompt, selectedIds, nodesById, focusId, edges }),
    [prompt, selectedIds, nodesById, focusId, edges],
  );

  if (selectedIds.length === 0) return null;

  const handleSend = async () => {
    if (!targetPaneId) return;
    setBusy(true);
    try {
      const url = `${baseUrl(api)}/api/panes/${encodeURIComponent(targetPaneId)}/send`;
      const res = await fetch(url, {
        method: "POST",
        headers: { "Content-Type": "application/json", ...authHeaders(api) },
        body: JSON.stringify({ text: composed, enter: true }),
      });
      if (!res.ok) throw new Error(`${res.status}`);
      setToast(`Sent ${selectedIds.length} node${selectedIds.length === 1 ? "" : "s"} to ${targetPaneId.slice(0, 7)}`);
      onSent();
      onClear();
    } catch (err) {
      setToast(err instanceof Error ? err.message : "send failed");
    } finally {
      setBusy(false);
    }
  };

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(composed);
      setToast(`Copied ${selectedIds.length} node${selectedIds.length === 1 ? "" : "s"}`);
    } catch {
      setToast("clipboard failed");
    }
  };

  useEffect(() => {
    if (!toast) return;
    const t = window.setTimeout(() => setToast(null), 2400);
    return () => window.clearTimeout(t);
  }, [toast]);

  return (
    <div className="absolute bottom-2 left-1/2 -translate-x-1/2 z-30 w-[640px] max-w-[calc(100%-1rem)] rounded-sm border border-[var(--ct-border)] bg-[var(--ct-bg)] shadow-lg p-2 flex flex-col gap-2">
      <div className="flex items-center justify-between text-xs text-[var(--ct-muted-fg)]">
        <div>
          <span className="text-[var(--ct-fg)] font-medium">{selectedIds.length}</span> node
          {selectedIds.length === 1 ? "" : "s"} selected
          <button
            type="button"
            className="ml-2 underline-offset-2 hover:underline"
            onClick={() => setShowList((s) => !s)}
          >
            {showList ? "hide list" : "show list"}
          </button>
        </div>
        <button
          type="button"
          className="text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]"
          onClick={onClear}
        >
          ✕ clear
        </button>
      </div>
      {showList && (
        <ul className="max-h-32 overflow-auto text-[10px] font-mono text-[var(--ct-muted-fg)] border border-[var(--ct-border)] rounded-sm bg-[var(--ct-active)]/30 p-1">
          {selectedIds.map((id) => {
            const n = nodesById.get(id);
            if (!n) return null;
            const loc = n.path && n.start_line != null ? `${n.path}:${n.start_line + 1}` : n.path ?? "-";
            return (
              <li key={id} className="truncate">
                {loc} — {n.display_name} ({n.kind})
              </li>
            );
          })}
        </ul>
      )}
      <div className="flex items-center gap-2 text-xs">
        <span className="text-[var(--ct-muted)]">Send to</span>
        <PanePicker panes={panes} selectedPaneId={targetPaneId} onChange={onPaneChange} />
      </div>
      <textarea
        rows={2}
        className="w-full text-xs bg-[var(--ct-active)]/30 border border-[var(--ct-border)] rounded-sm px-2 py-1 font-mono text-[var(--ct-fg)] outline-none focus:border-[var(--ct-accent)]"
        placeholder="Optional prompt — what should the agent do with these?"
        value={prompt}
        onChange={(e) => setPrompt(e.target.value)}
        onKeyDown={(e) => {
          if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.key === "Enter") {
            e.preventDefault();
            handleSend();
          }
        }}
      />
      <div className="flex items-center justify-between gap-2">
        <button
          type="button"
          className="text-xs px-2 py-1 rounded-sm border border-[var(--ct-border)] hover:bg-[var(--ct-hover)] text-[var(--ct-muted-fg)]"
          onClick={handleCopy}
        >
          📋 Copy as block
        </button>
        <div className="flex items-center gap-1">
          <span className="text-[10px] text-[var(--ct-muted)]">⇧⌘↵</span>
          <button
            type="button"
            disabled={!targetPaneId || busy}
            className="text-xs px-3 py-1 rounded-sm bg-[var(--ct-accent)] text-[var(--ct-bg)] disabled:opacity-50 disabled:cursor-not-allowed"
            onClick={handleSend}
          >
            {busy ? "Sending…" : "Send"}
          </button>
        </div>
      </div>
      {toast && (
        <div className="text-[10px] text-[var(--ct-muted-fg)] border-t border-[var(--ct-border)] pt-1">
          {toast}
        </div>
      )}
    </div>
  );
}
