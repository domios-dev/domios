import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ApiConfig } from "../../lib/types";
import type { MemEdge, MemNode } from "./types";
import { colorForKind } from "./nodeKind";
import { deleteItem, fetchItemFull, patchItem, type ItemPatch } from "./memoryApi";
import { UI_BUTTON, UI_INPUT, UI_TEXT } from "../shared/design";

interface Props {
  api: ApiConfig;
  node: MemNode;
  onUpdate: (id: string, patch: Partial<MemNode>) => void;
  onDelete: (id: string) => void;
  edges: MemEdge[];
  nodes: Map<string, MemNode>;
  onSelectNode: (id: string) => void;
}

interface Neighbor {
  id: string;
  rel: string;
  dir: "out" | "in";
  node: MemNode | null;
}

interface EditState {
  title: string;
  summary: string;
  body: string;
  area: string;
  importance: number;
  tags: string[];
}

function buildEditState(node: MemNode): EditState {
  return {
    title: node.title,
    summary: node.summary ?? "",
    body: node.body ?? "",
    area: node.area ?? "",
    importance: node.importance ?? 0,
    tags: [...(node.tags ?? [])],
  };
}

export function MemoryNodeDetail({
  api,
  node,
  onUpdate,
  onDelete,
  edges,
  nodes,
  onSelectNode,
}: Props) {
  const [tucked, setTucked] = useState(false);
  const neighbors: Neighbor[] = useMemo(() => {
    const out: Neighbor[] = [];
    for (const e of edges) {
      if (e.from === node.id) {
        out.push({ id: e.to, rel: e.rel, dir: "out", node: nodes.get(e.to) ?? null });
      } else if (e.to === node.id) {
        out.push({ id: e.from, rel: e.rel, dir: "in", node: nodes.get(e.from) ?? null });
      }
    }
    return out.sort((a, b) => {
      if (a.rel !== b.rel) return a.rel.localeCompare(b.rel);
      return a.id.localeCompare(b.id);
    });
  }, [edges, nodes, node.id]);
  const [editing, setEditing] = useState(false);
  const [edit, setEdit] = useState<EditState>(() => buildEditState(node));
  const [tagInput, setTagInput] = useState("");
  const [saveError, setSaveError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [loadingBody, setLoadingBody] = useState(false);
  // Local body cache — the source of truth for display. Decoupled from the
  // parent `node`, so a no-op onUpdate (graph canvas) can't trap this in a
  // refetch loop and the fetched body still renders.
  const [bodyText, setBodyText] = useState<string | null>(node.body ?? null);
  const onUpdateRef = useRef(onUpdate);
  onUpdateRef.current = onUpdate;

  useEffect(() => {
    setEdit(buildEditState(node));
    setEditing(false);
    setSaveError(null);
    setTucked(false);
    setBodyText(node.body ?? null);
  }, [node.id]);

  // Keep local cache in sync if the parent supplies the body later.
  useEffect(() => {
    if (node.body != null) setBodyText(node.body);
  }, [node.body]);

  // Lazy-load the body exactly once. Don't gate on `has_body` — the API can
  // return it null even when a body exists; a 404 (graph entity) resolves to "".
  useEffect(() => {
    if (bodyText != null) return;
    let cancelled = false;
    setLoadingBody(true);
    fetchItemFull(api, node.id)
      .then((item) => {
        if (cancelled) return;
        const b = item.body ?? "";
        setBodyText(b);
        setEdit((e) => ({ ...e, body: b }));
        onUpdateRef.current(node.id, {
          body: b,
          created_at: item.created_at,
          updated_at: item.updated_at,
        });
      })
      .catch(() => {
        if (!cancelled) setBodyText("");
      })
      .finally(() => {
        if (!cancelled) setLoadingBody(false);
      });
    return () => {
      cancelled = true;
    };
  }, [api, node.id, bodyText]);

  const handleSave = useCallback(async () => {
    const patch: ItemPatch = {};
    if (edit.title !== node.title) patch.title = edit.title;
    if (edit.summary !== (node.summary ?? "")) patch.summary = edit.summary;
    if (edit.body !== (node.body ?? "")) patch.body = edit.body;
    if (edit.area !== (node.area ?? "")) patch.area = edit.area;
    if (edit.importance !== (node.importance ?? 0)) patch.importance = edit.importance;
    const origTags = node.tags ?? [];
    const tagsChanged =
      origTags.length !== edit.tags.length ||
      origTags.some((t, i) => t !== edit.tags[i]);
    if (tagsChanged) patch.tags = edit.tags;

    try {
      await patchItem(api, node.id, patch);
      onUpdate(node.id, {
        title: edit.title,
        summary: edit.summary || null,
        body: edit.body,
        area: edit.area || null,
        importance: edit.importance,
        tags: edit.tags,
      });
      setEditing(false);
      setSaveError(null);
    } catch (e) {
      setSaveError(String(e));
    }
  }, [api, edit, node, onUpdate]);

  const handleCancel = useCallback(() => {
    setEdit(buildEditState(node));
    setEditing(false);
    setSaveError(null);
  }, [node]);

  const handleDelete = useCallback(async () => {
    if (!window.confirm(`Delete "${node.title}"?`)) return;
    try {
      await deleteItem(api, node.id);
      onDelete(node.id);
    } catch (e) {
      setSaveError(String(e));
    }
  }, [api, node.id, node.title, onDelete]);

  const handleCopyId = useCallback(() => {
    navigator.clipboard.writeText(node.id).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1000);
    });
  }, [node.id]);

  const addTag = useCallback(() => {
    const t = tagInput.trim();
    if (!t || edit.tags.includes(t)) return;
    setEdit((e) => ({ ...e, tags: [...e.tags, t] }));
    setTagInput("");
  }, [tagInput, edit.tags]);

  const removeTag = useCallback((tag: string) => {
    setEdit((e) => ({ ...e, tags: e.tags.filter((t) => t !== tag) }));
  }, []);

  const onTagKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLInputElement>) => {
      if (e.key === "Enter" || e.key === ",") {
        e.preventDefault();
        addTag();
      } else if (e.key === "Backspace" && tagInput === "" && edit.tags.length > 0) {
        e.preventDefault();
        setEdit((ed) => ({ ...ed, tags: ed.tags.slice(0, -1) }));
      }
    },
    [addTag, tagInput, edit.tags.length],
  );

  const inputClass = `${UI_INPUT.full} text-[10px] text-inherit`;
  const btnClass = `${UI_BUTTON.ghost} ${UI_BUTTON.compact} text-[11px]`;
  const sectionLabelClass = UI_TEXT.sectionLabel;
  const rowButtonClass = `${UI_BUTTON.base} ${UI_BUTTON.compact} flex items-center gap-1.5 text-left w-full hover:bg-[var(--ct-hover)]`;
  const mutedTextClass = "text-[10px] opacity-50";

  return (
    <>
      {tucked && (
        <button
          data-testid="memory-node-detail-show"
          onClick={() => setTucked(false)}
          aria-label="Show details"
          title="Show details"
          className="absolute top-3 right-0 z-10 px-1.5 py-2 rounded-l bg-[var(--ct-surface)] border border-r-0 border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] cursor-pointer text-xs leading-none"
        >
          ‹
        </button>
      )}
    <div
      data-testid="memory-node-detail"
      data-memory-id={node.id}
      data-memory-kind={node.kind}
      className="absolute top-0 right-0 bottom-0 w-[360px] p-4 z-10 overflow-y-auto flex flex-col gap-2.5 text-xs bg-[var(--ct-surface)] border-l border-[var(--ct-border)] text-[var(--ct-fg)] transition-transform duration-200 ease-out"
      style={{ transform: tucked ? "translateX(100%)" : "translateX(0)" }}
      onMouseDown={(e) => e.stopPropagation()}
      onWheel={(e) => e.stopPropagation()}
    >
      {saveError && (
        <div className="bg-[#ef4444] text-white px-2 py-1.5 rounded" data-testid="memory-node-detail-error" role="alert">{saveError}</div>
      )}

      <div className="flex items-start gap-2">
        <div
          className="w-2.5 h-2.5 mt-[5px] rounded-full shrink-0"
          style={{ background: colorForKind(node.kind) }}
        />
        {editing ? (
          <input
            data-testid="memory-node-title-input"
            className={`${inputClass} text-sm font-semibold`}
            value={edit.title}
            onChange={(e) => setEdit((s) => ({ ...s, title: e.target.value }))}
          />
        ) : (
          <strong className="text-sm flex-1 break-words">{node.title}</strong>
        )}
        <button
          data-testid="memory-node-detail-tuck"
          onClick={() => setTucked(true)}
          aria-label="Tuck panel"
          title="Tuck panel"
          className={`${btnClass} !px-2 !py-0.5 !text-base leading-none`}
        >
          ›
        </button>
      </div>

      <div className="flex gap-1.5 flex-wrap text-[10px] opacity-80">
        <span
          className="px-1.5 py-px rounded-[3px] font-semibold text-black"
          style={{ background: colorForKind(node.kind) }}
        >
          {node.kind}
        </span>
        {node.area && !editing && (
          <span className="px-1.5 py-px rounded-[3px] bg-[var(--ct-hover)]">{node.area}</span>
        )}
      </div>

      <div>
        <div className={sectionLabelClass}>Summary</div>
        {editing ? (
          <textarea
            rows={3}
            data-testid="memory-node-summary-input"
            className={`${inputClass} resize-y`}
            value={edit.summary}
            onChange={(e) => setEdit((s) => ({ ...s, summary: e.target.value }))}
          />
        ) : (
          <div className="whitespace-pre-wrap opacity-90">{node.summary ?? "—"}</div>
        )}
      </div>

      <div>
        <div className={sectionLabelClass}>
          Body {loadingBody && <span>(loading…)</span>}
        </div>
        {editing ? (
          <textarea
            rows={10}
            data-testid="memory-node-body-input"
            className={`${inputClass} resize-y font-mono`}
            value={edit.body}
            onChange={(e) => setEdit((s) => ({ ...s, body: e.target.value }))}
          />
        ) : (
          <pre className="m-0 whitespace-pre-wrap font-mono text-[11px] bg-[var(--ct-bg)] p-2 rounded max-h-[240px] overflow-y-auto">
            {bodyText ?? (loadingBody ? "" : "—")}
          </pre>
        )}
      </div>

      {editing && (
        <div>
          <div className={sectionLabelClass}>Area</div>
          <input
            data-testid="memory-node-area-input"
            className={inputClass}
            value={edit.area}
            onChange={(e) => setEdit((s) => ({ ...s, area: e.target.value }))}
          />
        </div>
      )}

      <div>
        <div className={sectionLabelClass}>
          Importance: {editing ? edit.importance.toFixed(1) : (node.importance ?? 0).toFixed(1)}
        </div>
        {editing ? (
          <input
            type="range"
            data-testid="memory-node-importance-input"
            min={0}
            max={10}
            step={0.1}
            value={edit.importance}
            onChange={(e) => setEdit((s) => ({ ...s, importance: Number(e.target.value) }))}
            className="w-full"
          />
        ) : null}
      </div>

      <div>
        <div className={sectionLabelClass}>Tags</div>
        <div className="flex flex-wrap gap-1">
          {(editing ? edit.tags : node.tags ?? []).map((t) => (
            <span
              key={t}
              className="px-1.5 py-0.5 bg-[var(--ct-hover)] rounded text-[10px] inline-flex items-center gap-1"
            >
              {t}
              {editing && (
                <button
                  data-testid="memory-node-tag-remove"
                  data-memory-tag={t}
                  onClick={() => removeTag(t)}
                  className="bg-transparent border-0 text-inherit p-0 text-xs leading-none cursor-pointer"
                >
                  ×
                </button>
              )}
            </span>
          ))}
          {editing && (
            <input
              placeholder="add tag…"
              data-testid="memory-node-tag-input"
              value={tagInput}
              onChange={(e) => setTagInput(e.target.value)}
              onKeyDown={onTagKeyDown}
              onBlur={addTag}
              className={`${inputClass} !w-[100px] !text-[10px]`}
            />
          )}
        </div>
      </div>

      {!editing && (
        <div>
          <div className={sectionLabelClass}>
            Related ({neighbors.length})
          </div>
          {neighbors.length === 0 ? (
            <div className={mutedTextClass}>orphan — no edges</div>
          ) : (
            <div className="flex flex-col gap-0.5 max-h-[260px] overflow-y-auto">
              {neighbors.map((nb, i) => (
                <button
                  key={`${nb.dir}-${nb.id}-${nb.rel}-${i}`}
                  data-testid="memory-node-related-item"
                  data-memory-id={nb.id}
                  data-memory-rel={nb.rel}
                  onClick={() => onSelectNode(nb.id)}
                  className={`${rowButtonClass} cursor-pointer`}
                  title={nb.id}
                >
                  <span
                    className="w-1.5 h-1.5 rounded-full shrink-0"
                    style={{ background: nb.node ? colorForKind(nb.node.kind) : "#666" }}
                  />
                  <span className="opacity-60 text-[9px] uppercase tracking-wide w-3 shrink-0">
                    {nb.dir === "out" ? "→" : "←"}
                  </span>
                  <span className="opacity-70 text-[10px] shrink-0 italic">
                    {nb.rel}
                  </span>
                  <span className="opacity-90 truncate flex-1 min-w-0">
                    {nb.node?.title ?? nb.id}
                  </span>
                </button>
              ))}
            </div>
          )}
        </div>
      )}

      {!editing && (
        <div>
          <div className={sectionLabelClass}>Stats</div>
          <div className="flex flex-wrap gap-x-3 gap-y-0.5 text-[10px] opacity-75">
            <span title="Has vector embedding for semantic search">
              embedding: {node.has_vector ? "✓" : "—"}
            </span>
            <span title="Has stored body content (vs entity-only)">
              body: {node.has_body ? "✓" : "—"}
            </span>
            {typeof node.access_count === "number" && (
              <span title="How many times this item has been read">
                reads: {node.access_count}
              </span>
            )}
            {typeof node.recurrence_count === "number" && node.recurrence_count > 0 && (
              <span title="How many similar saves have been deduped into this">
                merges: {node.recurrence_count}
              </span>
            )}
          </div>
        </div>
      )}

      {(node.created_at || node.updated_at) && !editing && (
        <div className={mutedTextClass}>
          {node.created_at && <div>created: {node.created_at}</div>}
          {node.updated_at && <div>updated: {node.updated_at}</div>}
        </div>
      )}

      <div className={`${mutedTextClass} break-all`}>{node.id}</div>

      <div className="flex gap-1.5 mt-auto flex-wrap">
        {editing ? (
          <>
            <button onClick={handleSave} className={`${btnClass} !bg-[#3b82f6] !border-[#3b82f6]`} data-testid="memory-node-save">
              Save
            </button>
            <button onClick={handleCancel} className={btnClass} data-testid="memory-node-cancel">
              Cancel
            </button>
          </>
        ) : (
          <>
            <button onClick={() => setEditing(true)} className={btnClass} data-testid="memory-node-edit">
              Edit
            </button>
            <button
              onClick={handleDelete}
              data-testid="memory-node-delete"
              className={`${btnClass} !text-[#ef4444] !border-[#ef4444]`}
            >
              Delete
            </button>
            <button onClick={handleCopyId} className={btnClass} data-testid="memory-node-copy-id">
              {copied ? "Copied!" : "Copy ID"}
            </button>
          </>
        )}
      </div>
    </div>
    </>
  );
}
