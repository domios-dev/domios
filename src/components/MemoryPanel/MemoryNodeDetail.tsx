import { useCallback, useEffect, useMemo, useState } from "react";
import type { ApiConfig } from "../../lib/types";
import type { MemEdge, MemNode } from "./types";
import { colorForKind } from "./nodeKind";
import { deleteItem, fetchItemFull, patchItem, type ItemPatch } from "./memoryApi";

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

  useEffect(() => {
    setEdit(buildEditState(node));
    setEditing(false);
    setSaveError(null);
    setTucked(false);
  }, [node.id]);

  useEffect(() => {
    if (node.body != null || !node.has_body) return;
    let cancelled = false;
    setLoadingBody(true);
    fetchItemFull(api, node.id)
      .then((item) => {
        if (cancelled) return;
        onUpdate(node.id, {
          body: item.body,
          created_at: item.created_at,
          updated_at: item.updated_at,
        });
      })
      .catch(() => {})
      .finally(() => {
        if (!cancelled) setLoadingBody(false);
      });
    return () => {
      cancelled = true;
    };
  }, [api, node.id, node.body, node.has_body, onUpdate]);

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

  const inputClass =
    "w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded text-inherit px-1.5 py-1 text-xs font-[inherit] box-border";
  const btnClass =
    "bg-transparent border border-[var(--ct-border)] text-inherit px-2.5 py-1 rounded text-[11px] cursor-pointer";

  return (
    <>
      {tucked && (
        <button
          onClick={() => setTucked(false)}
          aria-label="Show details"
          title="Show details"
          className="absolute top-3 right-0 z-10 px-1.5 py-2 rounded-l bg-[var(--ct-surface)] border border-r-0 border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] cursor-pointer text-xs leading-none"
        >
          ‹
        </button>
      )}
    <div
      className="absolute top-0 right-0 bottom-0 w-[360px] p-4 z-10 overflow-y-auto flex flex-col gap-2.5 text-xs bg-[var(--ct-surface)] border-l border-[var(--ct-border)] text-[var(--ct-fg)] transition-transform duration-200 ease-out"
      style={{ transform: tucked ? "translateX(100%)" : "translateX(0)" }}
      onMouseDown={(e) => e.stopPropagation()}
      onWheel={(e) => e.stopPropagation()}
    >
      {saveError && (
        <div className="bg-[#ef4444] text-white px-2 py-1.5 rounded">{saveError}</div>
      )}

      <div className="flex items-start gap-2">
        <div
          className="w-2.5 h-2.5 mt-[5px] rounded-full shrink-0"
          style={{ background: colorForKind(node.kind) }}
        />
        {editing ? (
          <input
            className={`${inputClass} text-sm font-semibold`}
            value={edit.title}
            onChange={(e) => setEdit((s) => ({ ...s, title: e.target.value }))}
          />
        ) : (
          <strong className="text-sm flex-1 break-words">{node.title}</strong>
        )}
        <button
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
        <div className="text-[10px] opacity-60 mb-[3px]">Summary</div>
        {editing ? (
          <textarea
            rows={3}
            className={`${inputClass} resize-y`}
            value={edit.summary}
            onChange={(e) => setEdit((s) => ({ ...s, summary: e.target.value }))}
          />
        ) : (
          <div className="whitespace-pre-wrap opacity-90">{node.summary ?? "—"}</div>
        )}
      </div>

      <div>
        <div className="text-[10px] opacity-60 mb-[3px]">
          Body {loadingBody && <span>(loading…)</span>}
        </div>
        {editing ? (
          <textarea
            rows={10}
            className={`${inputClass} resize-y font-mono`}
            value={edit.body}
            onChange={(e) => setEdit((s) => ({ ...s, body: e.target.value }))}
          />
        ) : (
          <pre className="m-0 whitespace-pre-wrap font-mono text-[11px] bg-[var(--ct-bg)] p-2 rounded max-h-[240px] overflow-y-auto">
            {node.body ?? ""}
          </pre>
        )}
      </div>

      {editing && (
        <div>
          <div className="text-[10px] opacity-60 mb-[3px]">Area</div>
          <input
            className={inputClass}
            value={edit.area}
            onChange={(e) => setEdit((s) => ({ ...s, area: e.target.value }))}
          />
        </div>
      )}

      <div>
        <div className="text-[10px] opacity-60 mb-[3px]">
          Importance: {editing ? edit.importance.toFixed(1) : (node.importance ?? 0).toFixed(1)}
        </div>
        {editing ? (
          <input
            type="range"
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
        <div className="text-[10px] opacity-60 mb-[3px]">Tags</div>
        <div className="flex flex-wrap gap-1">
          {(editing ? edit.tags : node.tags ?? []).map((t) => (
            <span
              key={t}
              className="px-1.5 py-0.5 bg-[var(--ct-hover)] rounded text-[10px] inline-flex items-center gap-1"
            >
              {t}
              {editing && (
                <button
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
          <div className="text-[10px] opacity-60 mb-[3px]">
            Related ({neighbors.length})
          </div>
          {neighbors.length === 0 ? (
            <div className="text-[10px] opacity-50">orphan — no edges</div>
          ) : (
            <div className="flex flex-col gap-0.5 max-h-[260px] overflow-y-auto">
              {neighbors.map((nb, i) => (
                <button
                  key={`${nb.dir}-${nb.id}-${nb.rel}-${i}`}
                  onClick={() => onSelectNode(nb.id)}
                  className="flex items-center gap-1.5 text-left text-[11px] px-1.5 py-0.5 rounded hover:bg-[var(--ct-hover)] cursor-pointer w-full"
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
          <div className="text-[10px] opacity-60 mb-[3px]">Stats</div>
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
        <div className="text-[10px] opacity-50">
          {node.created_at && <div>created: {node.created_at}</div>}
          {node.updated_at && <div>updated: {node.updated_at}</div>}
        </div>
      )}

      <div className="text-[10px] opacity-50 break-all">{node.id}</div>

      <div className="flex gap-1.5 mt-auto flex-wrap">
        {editing ? (
          <>
            <button onClick={handleSave} className={`${btnClass} !bg-[#3b82f6] !border-[#3b82f6]`}>
              Save
            </button>
            <button onClick={handleCancel} className={btnClass}>
              Cancel
            </button>
          </>
        ) : (
          <>
            <button onClick={() => setEditing(true)} className={btnClass}>
              Edit
            </button>
            <button
              onClick={handleDelete}
              className={`${btnClass} !text-[#ef4444] !border-[#ef4444]`}
            >
              Delete
            </button>
            <button onClick={handleCopyId} className={btnClass}>
              {copied ? "Copied!" : "Copy ID"}
            </button>
          </>
        )}
      </div>
    </div>
    </>
  );
}
