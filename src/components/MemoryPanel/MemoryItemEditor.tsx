import { useCallback, useEffect, useRef, useState } from "react";
import type { ApiConfig } from "../../lib/types";
import { fetchItemFull, patchItem, deleteItem } from "./memoryApi";
import { colorForKind } from "./nodeKind";

interface Props {
  itemId: string;
  api: ApiConfig;
  onClose: () => void;
  onDeleted: (id: string) => void;
  onUpdated: (id: string) => void;
}

export function MemoryItemEditor({ itemId, api, onClose, onDeleted, onUpdated }: Props) {
  const [item, setItem] = useState<{
    id: string; kind: string; title: string; body: string;
    area?: string; tags?: string[]; importance?: number;
    created_at?: string; updated_at?: string; accessed_at?: string;
  } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [dirty, setDirty] = useState(false);
  const backdropRef = useRef<HTMLDivElement>(null);
  const bodyRef = useRef<HTMLTextAreaElement>(null);

  // Draft state for editable fields
  const [draftTitle, setDraftTitle] = useState("");
  const [draftBody, setDraftBody] = useState("");
  const [draftArea, setDraftArea] = useState("");
  const [draftTags, setDraftTags] = useState("");

  useEffect(() => {
    let active = true;
    fetchItemFull(api, itemId)
      .then(data => {
        if (!active) return;
        setItem(data as typeof item);
        setDraftTitle(data.title);
        setDraftBody(data.body || "");
        setDraftArea(data.area || "");
        setDraftTags((data.tags || []).join(", "));
      })
      .catch(e => { if (active) setError(e instanceof Error ? e.message : String(e)); });
    return () => { active = false; };
  }, [api, itemId]);

  // Track dirty
  useEffect(() => {
    if (!item) return;
    const d = draftTitle !== item.title
      || draftBody !== (item.body || "")
      || draftArea !== (item.area || "")
      || draftTags !== (item.tags || []).join(", ");
    setDirty(d);
  }, [item, draftTitle, draftBody, draftArea, draftTags]);

  const handleSave = useCallback(async () => {
    if (!item || !dirty) return;
    setSaving(true);
    try {
      const patch: Record<string, unknown> = {};
      if (draftTitle !== item.title) patch.title = draftTitle;
      if (draftBody !== (item.body || "")) patch.body = draftBody;
      if (draftArea !== (item.area || "")) patch.area = draftArea;
      const newTags = draftTags.split(",").map(t => t.trim()).filter(Boolean);
      if (newTags.join(",") !== (item.tags || []).join(",")) patch.tags = newTags;
      await patchItem(api, itemId, patch);
      onUpdated(itemId);
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  }, [api, itemId, item, dirty, draftTitle, draftBody, draftArea, draftTags, onUpdated, onClose]);

  const handleDelete = useCallback(async () => {
    try {
      await deleteItem(api, itemId);
      onDeleted(itemId);
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [api, itemId, onDeleted, onClose]);

  // Close on Escape (if not dirty)
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
      if (e.key === "s" && (e.metaKey || e.ctrlKey) && dirty) {
        e.preventDefault();
        handleSave();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose, dirty, handleSave]);

  const handleBackdropClick = (e: React.MouseEvent) => {
    if (e.target === backdropRef.current) onClose();
  };

  const color = item ? colorForKind(item.kind) : "#888";

  return (
    <div
      ref={backdropRef}
      onClick={handleBackdropClick}
      className="fixed inset-0 z-50 flex items-center justify-center"
      style={{ background: "rgba(0,0,0,0.4)" }}
      data-testid="memory-item-editor-backdrop"
    >
      <div
        className="flex flex-col bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded-lg shadow-2xl"
        style={{ width: "min(680px, 90vw)", height: "min(560px, 80vh)" }}
        data-testid="memory-item-editor"
        data-memory-id={itemId}
        role="dialog"
        aria-label="Edit memory item"
      >
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-2.5 border-b border-[var(--ct-border)]">
          <div className="flex items-center gap-2 min-w-0">
            <span className="w-2.5 h-2.5 rounded-full shrink-0" style={{ background: color }} />
            {item && (
              <span className="text-[10px] px-1.5 py-px rounded-[3px] font-semibold shrink-0" style={{ background: `${color}22`, color }}>
                {item.kind}
              </span>
            )}
            <span className="text-[10px] text-[var(--ct-muted-fg)] truncate">{itemId}</span>
          </div>
          <div className="flex items-center gap-2">
            {dirty && (
              <button
                data-testid="memory-item-save"
                onClick={handleSave}
                disabled={saving}
                className="bg-[var(--ct-accent)] text-white border-0 px-3 py-1 rounded text-[11px] cursor-pointer disabled:opacity-50"
              >
                {saving ? "Saving..." : "Save"}
              </button>
            )}
            <button
              data-testid="memory-item-close"
              onClick={onClose}
              className="bg-transparent border-0 text-[var(--ct-muted-fg)] cursor-pointer text-sm px-1 hover:text-[var(--ct-fg)]"
            >
              ✕
            </button>
          </div>
        </div>

        {error && (
          <div className="mx-4 mt-2 p-2 rounded bg-[var(--ct-red)]/20 text-[var(--ct-red)] text-[11px]" data-testid="memory-item-error" role="alert">
            {error}
          </div>
        )}

        {!item && !error && (
          <div className="flex-1 flex items-center justify-center text-[var(--ct-muted-fg)] text-xs" data-testid="memory-item-loading">Loading...</div>
        )}

        {item && (
          <div className="flex-1 overflow-y-auto px-4 py-3 space-y-3">
            {/* Title */}
            <div>
              <label className="text-[10px] text-[var(--ct-muted-fg)] block mb-1">Title</label>
              <input
                data-testid="memory-item-title-input"
                className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
                value={draftTitle}
                onChange={e => setDraftTitle(e.target.value)}
              />
            </div>

            {/* Area + Tags row */}
            <div className="flex gap-3">
              <div className="flex-1">
                <label className="text-[10px] text-[var(--ct-muted-fg)] block mb-1">Area</label>
                <input
                  data-testid="memory-item-area-input"
                  className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
                  value={draftArea}
                  onChange={e => setDraftArea(e.target.value)}
                  placeholder="auth, deploy, home..."
                />
              </div>
              <div className="flex-1">
                <label className="text-[10px] text-[var(--ct-muted-fg)] block mb-1">Tags</label>
                <input
                  data-testid="memory-item-tags-input"
                  className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
                  value={draftTags}
                  onChange={e => setDraftTags(e.target.value)}
                  placeholder="comma, separated, tags"
                />
              </div>
            </div>

            {/* Body */}
            <div className="flex-1">
              <label className="text-[10px] text-[var(--ct-muted-fg)] block mb-1">Body</label>
              <textarea
                ref={bodyRef}
                data-testid="memory-item-body-input"
                className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-2 text-[11px] text-[var(--ct-fg)] font-mono leading-relaxed focus:border-[var(--ct-accent)] focus:outline-none resize-y"
                style={{ minHeight: "200px" }}
                value={draftBody}
                onChange={e => setDraftBody(e.target.value)}
              />
            </div>

            {/* Metadata (read-only) */}
            <div className="flex items-center gap-4 text-[10px] text-[var(--ct-muted-fg)] pt-1 border-t border-[var(--ct-border)]">
              {item.created_at && <span>Created: {new Date(item.created_at).toLocaleString()}</span>}
              {item.updated_at && <span>Updated: {new Date(item.updated_at).toLocaleString()}</span>}
              {item.importance != null && <span>Importance: {item.importance}</span>}
              <button
                data-testid="memory-item-delete"
                onClick={handleDelete}
                className="ml-auto bg-transparent border border-[#ef4444] text-[#ef4444] px-2 py-0.5 rounded text-[10px] cursor-pointer hover:bg-[#ef444422]"
              >
                Delete
              </button>
            </div>
          </div>
        )}

        {/* Footer hint */}
        {dirty && (
          <div className="px-4 py-1.5 border-t border-[var(--ct-border)] text-[10px] text-[var(--ct-muted-fg)]">
            Cmd+S to save · Esc to close · body changes re-index vectors
          </div>
        )}
      </div>
    </div>
  );
}
