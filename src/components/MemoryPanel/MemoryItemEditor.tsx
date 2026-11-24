import { ChevronLeft, Trash2, X } from "lucide-react";
import { MemoryImportance, MemoryKindChip, MemoryProvenance } from "./MemoryProvenance";
import type { MemItemFull } from "./types";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useCallback, useEffect, useRef, useState } from "react";
import type { ApiConfig } from "../../lib/types";
import { fetchItemFull, patchItem, deleteItem } from "./memoryApi";
import { Button } from "../shared/Button";
import { UI_INPUT } from "../shared/design";
import { memoryDetailIdentity } from "./memoryDetailModel";
import { BusyState } from "../shared/BusyState";

export const memorySheet = {
  frame: "absolute inset-y-0 right-0 z-10 flex w-full flex-col bg-[var(--ct-surface)] text-[var(--ct-fg)] @min-[720px]/memory:w-[440px] @min-[720px]/memory:shadow-[-1px_0_0_var(--ct-border-subtle)]",
  header: "flex min-h-[52px] shrink-0 items-center gap-3 border-b border-[var(--ct-border-subtle)] px-3.5",
  body: "min-h-0 flex-1 space-y-4 overflow-y-auto px-4 py-[18px] @min-[720px]/memory:px-[22px]",
  footer: "flex shrink-0 flex-wrap items-center gap-x-2 gap-y-1 border-t border-[var(--ct-border-subtle)] px-3.5 py-2.5",
  label: "mb-1.5 block text-xs font-[550] text-[var(--ct-muted-fg)]",
  input: `${UI_INPUT.full} !h-8 !min-h-0 !rounded-lg !text-[13px]`,
  textarea: `${UI_INPUT.full} min-h-60 resize-y !rounded-lg !py-2.5 !text-[13px] !leading-[1.55]`,
  technical: "text-xs text-[var(--ct-muted-fg)] [&>summary]:cursor-pointer",
} as const;

interface Props {
  itemId: string;
  api: ApiConfig;
  onClose: () => void;
  onDeleted: (id: string) => void;
  onUpdated: (id: string) => void;
}

export function MemoryItemEditor({ itemId, api, onClose, onDeleted, onUpdated }: Props) {
  const [item, setItem] = useState<MemItemFull | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [dirty, setDirty] = useState(false);
  const backdropRef = useRef<HTMLDivElement>(null);
  const bodyRef = useRef<HTMLTextAreaElement>(null);

  const [draftTitle, setDraftTitle] = useState("");
  const [draftBody, setDraftBody] = useState("");
  const [draftArea, setDraftArea] = useState("");
  const [draftTags, setDraftTags] = useState("");

  useEffect(() => {
    let active = true;
    fetchItemFull(api, itemId)
      .then(data => {
        if (!active) return;
        setItem(data);
        setDraftTitle(data.title);
        setDraftBody(data.body || "");
        setDraftArea(data.area || "");
        setDraftTags((data.tags || []).join(", "));
      })
      .catch(e => { if (active) setError(e instanceof Error ? e.message : String(e)); });
    return () => { active = false; };
  }, [api.host, api.port, api.token, itemId]);

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
  }, [api.host, api.port, api.token, itemId, item, dirty, draftTitle, draftBody, draftArea, draftTags, onUpdated, onClose]);

  const handleDelete = useCallback(async () => {
    try {
      await deleteItem(api, itemId);
      onDeleted(itemId);
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [api.host, api.port, api.token, itemId, onDeleted, onClose]);

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

  const identity = memoryDetailIdentity(item);
  return (
    <div
      ref={backdropRef}
      onClick={handleBackdropClick}
      className="absolute inset-0 z-50"
      data-testid="memory-item-editor-backdrop"
    >
      <div
        className={memorySheet.frame}
        data-testid="memory-item-editor"
        data-memory-id={itemId}
        role="dialog"
        aria-label="Edit memory item"
      >
        <div className={memorySheet.header}>
          <Button variant="icon" size="sm" aria-label="Back to Memory Inspect" title="Back to Memory Inspect" data-testid="memory-item-back" onClick={onClose}>
            <ChevronLeft size={16} className="@min-[720px]/memory:hidden" aria-hidden="true" />
            <X size={16} className="hidden @min-[720px]/memory:block" aria-hidden="true" />
          </Button>
          {item && <MemoryKindChip kind={item.kind} />}
          <h2 className="sr-only" data-testid="memory-item-heading">{identity.title}</h2>
        </div>

        {error && (
          <div data-testid="memory-item-error"><InlineErrorNotice error={error} operation="saving this memory" blocking /></div>
        )}

        {!item && !error && (
          <div className="flex-1 flex items-center justify-center text-xs" data-testid="memory-item-loading">
            <BusyState labelKey="memPending" inline />
          </div>
        )}

        {item && (
          <div className={memorySheet.body}>
            <div className="flex flex-wrap items-center gap-x-3.5 gap-y-1 text-xs text-[var(--ct-muted-fg)] tabular-nums">
              <MemoryProvenance author={item.author} createdAt={item.created_at} />
              {item.updated_at && <span title={new Date(item.updated_at).toLocaleString()}>Updated: {new Date(item.updated_at).toLocaleDateString()}</span>}
              {item.created_at && <span title={new Date(item.created_at).toLocaleString()}>Created: {new Date(item.created_at).toLocaleDateString()}</span>}
              {item.importance != null && <MemoryImportance value={item.importance} />}
            </div>
            <label className="block">
              <span className={memorySheet.label}>Title</span>
              <input
                data-testid="memory-item-title-input"
                className={`${UI_INPUT.full} !h-10 !rounded-lg !text-base font-semibold`}
                value={draftTitle}
                onChange={e => setDraftTitle(e.target.value)}
              />
            </label>

            <div className="grid gap-4 @min-[720px]/memory:grid-cols-2">
              <label className="min-w-0">
                <span className={memorySheet.label}>Area</span>
                <input
                  data-testid="memory-item-area-input"
                  className={memorySheet.input}
                  value={draftArea}
                  onChange={e => setDraftArea(e.target.value)}
                  placeholder="auth, deploy, home..."
                />
              </label>
              <label className="min-w-0">
                <span className={memorySheet.label}>Tags</span>
                <input
                  data-testid="memory-item-tags-input"
                  className={memorySheet.input}
                  value={draftTags}
                  onChange={e => setDraftTags(e.target.value)}
                  placeholder="comma, separated, tags"
                />
              </label>
            </div>

            <label className="block">
              <span className={memorySheet.label}>Body</span>
              <textarea
                ref={bodyRef}
                data-testid="memory-item-body-input"
                className={memorySheet.textarea}
                value={draftBody}
                onChange={e => setDraftBody(e.target.value)}
              />
            </label>
            <details className={memorySheet.technical}><summary>Technical</summary><div className="mt-2 break-all font-mono">{item.id}</div></details>
          </div>
        )}

        {item && (
          <div className={memorySheet.footer}>
            <Button size="sm" className="!text-[var(--ct-red)]" data-testid="memory-item-delete" onClick={handleDelete}>
              <Trash2 size={14} aria-hidden="true" /> Delete
            </Button>
            {dirty && (
              <>
                <span className="ml-auto text-xs text-[var(--ct-muted-fg)]">Unsaved changes</span>
                <Button variant="primary" size="sm" data-testid="memory-item-save" onClick={handleSave} disabled={saving}>
                  {saving ? "Saving..." : "Save"}
                </Button>
                <span className="basis-full px-1 text-[10px] text-[var(--ct-muted-fg)]">
                  Cmd+S to save · Esc to close · body changes re-index vectors
                </span>
              </>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
