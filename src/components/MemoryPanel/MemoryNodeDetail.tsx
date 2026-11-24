import { ChevronLeft, Copy, Trash2, X } from "lucide-react";
import Markdown from "react-markdown";
import { MemoryDetailMetadata, MemoryImportance, MemoryKindChip } from "./MemoryProvenance";
import { memorySheet, useCopyMemoryId } from "./MemoryItemEditor";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { BusyState } from "../shared/BusyState";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ApiConfig } from "../../lib/types";
import type { MemEdge, MemItemFull, MemNode } from "./types";
import { colorForKind } from "./nodeKind";
import { deleteItem, fetchItemFull, patchItem, type ItemPatch } from "./memoryApi";
import { Button } from "../shared/Button";
import { Chip } from "../shared/Chip";

interface Props {
  api: ApiConfig;
  node: MemNode;
  onUpdate: (id: string, patch: Partial<MemNode>) => void;
  onDelete: (id: string) => void;
  edges: MemEdge[];
  nodes: Map<string, MemNode>;
  onSelectNode: (id: string) => void;
  onUnpinPosition?: (id: string) => Promise<void>;
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
  node: initialNode,
  onUpdate,
  onDelete,
  edges,
  nodes,
  onSelectNode,
  onUnpinPosition,
}: Props) {
  const [loadedItem, setLoadedItem] = useState<Pick<MemItemFull, "id" | "author" | "created_at" | "updated_at"> | null>(null);
  const node = loadedItem?.id === initialNode.id ? { ...loadedItem, ...initialNode, author: initialNode.author ?? loadedItem.author, created_at: initialNode.created_at ?? loadedItem.created_at, updated_at: initialNode.updated_at ?? loadedItem.updated_at } : initialNode;
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
  const { copyId, copyLabel } = useCopyMemoryId(node.id);
  const [loadingBody, setLoadingBody] = useState(false);
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

  // return it null even when a body exists; a 404 (graph entity) resolves to "".
  useEffect(() => {
    if (bodyText != null) return;
    let cancelled = false;
    setLoadingBody(true);
    fetchItemFull(api, node.id)
      .then((item) => {
        if (cancelled) return;
        setLoadedItem(item);
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
  }, [api.host, api.port, api.token, node.id, bodyText]);

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
  }, [api.host, api.port, api.token, edit, node, onUpdate]);

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
  }, [api.host, api.port, api.token, node.id, node.title, onDelete]);

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

  const mutedTextClass = "text-xs text-[var(--ct-muted-fg)]";

  return (
    <>
      {tucked && (
        <button
          data-testid="memory-node-detail-show"
          onClick={() => setTucked(false)}
          aria-label="Show details"
          title="Show details"
          className="absolute top-3 right-0 z-10 px-1.5 py-2 rounded-l bg-[var(--ct-surface)] border border-r-0 border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] cursor-pointer text-xs leading-none @max-[720px]/memory:z-30"
        >
          ‹
        </button>
      )}
      <div
        data-testid="memory-node-detail"
        data-memory-id={node.id}
        data-memory-kind={node.kind}
        className={`${memorySheet.frame} @max-[720px]/memory:z-30 motion-safe:transition-transform motion-safe:duration-200 motion-safe:ease-out`}
        style={{ transform: tucked ? "translateX(100%)" : "translateX(0)" }}
        onMouseDown={(e) => e.stopPropagation()}
        onWheel={(e) => e.stopPropagation()}
      >
        {saveError && (
          <div data-testid="memory-node-detail-error"><InlineErrorNotice error={saveError} operation="saving this memory" blocking /></div>
        )}

        <div className={memorySheet.header}>
          <Button
            variant="icon"
            size="sm"
            data-testid="memory-node-detail-tuck"
            onClick={() => setTucked(true)}
            aria-label="Hide memory details"
            title="Hide memory details"
          >
            <ChevronLeft size={16} className="@min-[720px]/memory:hidden" aria-hidden="true" />
            <X size={16} className="hidden @min-[720px]/memory:block" aria-hidden="true" />
          </Button>
          <MemoryKindChip kind={node.kind} />
          {!editing && (
            <div className="ml-auto flex items-center gap-2">
              <Button variant="icon" size="sm" onClick={copyId} aria-label={copyLabel} title={copyLabel} data-testid="memory-node-copy-id">
                <Copy size={15} aria-hidden="true" />
              </Button>
              <Button size="sm" className="!border !border-[var(--ct-border-subtle)]" onClick={() => setEditing(true)} data-testid="memory-node-edit">
                Edit
              </Button>
            </div>
          )}
        </div>

        <div className={`${memorySheet.body} space-y-4 text-[13px]`}>
          {!editing && <h2 className="break-words text-lg font-[650]">{node.title}</h2>}
          <MemoryDetailMetadata author={node.author} updatedAt={node.updated_at} createdAt={node.created_at} />
          {editing && (
            <label className="block">
              <span className={memorySheet.label}>Title</span>
              <input
                data-testid="memory-node-title-input"
                className={`${memorySheet.input} !h-10 !text-base font-semibold`}
                value={edit.title}
                onChange={(e) => setEdit((s) => ({ ...s, title: e.target.value }))}
              />
            </label>
          )}
          {(editing || node.summary?.trim()) && <div>
            <div className={memorySheet.label}>Summary</div>
            {editing ? (
              <textarea
                rows={3}
                data-testid="memory-node-summary-input"
                className={`${memorySheet.textarea} !min-h-0`}
                value={edit.summary}
                onChange={(e) => setEdit((s) => ({ ...s, summary: e.target.value }))}
              />
            ) : (
              <div className="whitespace-pre-wrap opacity-90">{node.summary}</div>
            )}
          </div>}

          <div>
            {loadingBody && <BusyState label="Loading memory" inline className={memorySheet.label} />}
            {editing ? (
              <label className="block">
                <span className={memorySheet.label}>Body</span>
                <textarea
                  rows={10}
                  data-testid="memory-node-body-input"
                  className={memorySheet.textarea}
                  style={{ minHeight: 240 }}
                  value={edit.body}
                  onChange={(e) => setEdit((s) => ({ ...s, body: e.target.value }))}
                />
              </label>
            ) : (
              <div
                data-testid="memory-node-body-preview"
                className="break-words text-[13px] leading-[1.55] [&_p]:mb-3 [&_p:last-child]:mb-0"
              >
                <Markdown components={{ img: ({ alt }) => <span>{alt}</span>, code: ({ children }) => <code className="font-mono text-[13px] bg-[var(--ct-bg)] rounded px-1">{children}</code>, pre: ({ children }) => <pre className="whitespace-pre-wrap bg-[var(--ct-bg)] p-2 rounded">{children}</pre> }}>{bodyText ?? ""}</Markdown>
              </div>
            )}
          </div>

          <div className="grid grid-cols-2 items-start gap-4">
            {editing ? (
              <label className="min-w-0">
                <span className={memorySheet.label}>Area</span>
                <input
                  data-testid="memory-node-area-input"
                  className={memorySheet.input}
                  value={edit.area}
                  onChange={(e) => setEdit((s) => ({ ...s, area: e.target.value }))}
                />
              </label>
            ) : node.area && (
              <div className="min-w-0 break-words">
                <span className={memorySheet.label}>Area</span>
                {node.area}
              </div>
            )}
            <div className={mutedTextClass}>
              {editing ? (
                <label className="block">
                  <span className={memorySheet.label}>Importance</span>
                  <input
                    type="range"
                    data-testid="memory-node-importance-input"
                    min={0}
                    max={5}
                    step={0.1}
                    value={edit.importance}
                    onChange={(e) => setEdit((s) => ({ ...s, importance: Number(e.target.value) }))}
                    className="w-full"
                  />
                  <span>{edit.importance} of 5</span>
                </label>
              ) : <MemoryImportance value={node.importance ?? 0} />}
            </div>
          </div>

          {(editing || (node.tags?.length ?? 0) > 0) && <div>
            <div className={memorySheet.label}>Tags</div>
            <div className="flex flex-wrap gap-1">
              {(editing ? edit.tags : node.tags ?? []).map((t) => (
                <Chip
                  key={t}
                  className="!h-[22px] !gap-1 !rounded-md !border-0 !bg-[var(--ct-fg)]/7 !px-2 !text-xs"
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
                </Chip>
              ))}
              {editing && (
                <input
                  placeholder="add tag…"
                  data-testid="memory-node-tag-input"
                  value={tagInput}
                  onChange={(e) => setTagInput(e.target.value)}
                  onKeyDown={onTagKeyDown}
                  onBlur={addTag}
                  className={`${memorySheet.input} !w-28 !text-xs`}
                />
              )}
            </div>
          </div>}

          {!editing && neighbors.length > 0 && (
            <div>
              <div className={memorySheet.label}>
                Related ({neighbors.length})
              </div>
              {(
                <div className="flex flex-col gap-0.5 max-h-[260px] overflow-y-auto">
                  {neighbors.map((nb, i) => (
                    <button
                      key={`${nb.dir}-${nb.id}-${nb.rel}-${i}`}
                      data-testid="memory-node-related-item"
                      data-memory-id={nb.id}
                      data-memory-rel={nb.rel}
                      onClick={() => onSelectNode(nb.id)}
                      className="flex w-full cursor-pointer items-center gap-2 rounded-lg px-2 py-1.5 text-left hover:bg-[var(--ct-bg-hover)]"
                      title={nb.id}
                    >
                      <span
                        className="w-1.5 h-1.5 rounded-full shrink-0"
                        style={{ background: nb.node ? colorForKind(nb.node.kind) : "var(--ct-muted-fg)" }}
                      />
                      <span className="sr-only">
                        {nb.dir === "out" ? "→" : "←"}
                      </span>
                      <span className="min-w-0 flex-1 truncate">
                        {nb.node?.title ?? nb.id}
                      </span>
                      <span className="max-w-[40%] shrink-0 truncate text-xs text-[var(--ct-muted-fg)]">
                        {nb.rel}
                      </span>
                    </button>
                  ))}
                </div>
              )}
            </div>
          )}

          {!editing && (
            <details className={memorySheet.technical}>
              <summary>Technical</summary>
              <div className={`${mutedTextClass} mb-2 break-all font-mono`}>{node.id}</div>
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
              {(node.created_at || node.updated_at) && (
                <div className={mutedTextClass}>
                  {node.created_at && <div>created: {node.created_at}</div>}
                  {node.updated_at && <div>updated: {node.updated_at}</div>}
                </div>
              )}
            </details>
          )}
        </div>

        <div className={memorySheet.footer}>
          {editing ? (
            <>
              <Button onClick={handleCancel} size="sm" className="ml-auto" data-testid="memory-node-cancel">
                Cancel
              </Button>
              <Button onClick={handleSave} variant="primary" size="sm" data-testid="memory-node-save">
                Save
              </Button>
            </>
          ) : (
            <>
              <Button
                onClick={handleDelete}
                size="sm"
                data-testid="memory-node-delete"
                className="gap-1.5 !text-[var(--ct-red)]"
              >
                <Trash2 size={14} aria-hidden="true" /> Delete
              </Button>
              {node.pinned && onUnpinPosition && (
                <Button onClick={() => { void onUnpinPosition(node.id).catch(error => setSaveError(String(error))); }} size="sm" className="ml-auto">
                  Unpin position
                </Button>
              )}
            </>
          )}
        </div>
      </div>
    </>
  );
}
