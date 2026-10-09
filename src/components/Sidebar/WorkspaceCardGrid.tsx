import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { ChevronUp, Pencil, Plus } from "lucide-react";
import { ActivityDots } from "../shared/ActivityDots";
import type { TabStatus, Workspace } from "../../lib/types";
import { SortableContext, SortableRow, WS_PREFIX, rectSortingStrategy } from "./sortable";
import {
  defaultWorkspaceInitials,
  loadWorkspaceInitials,
  normalizeWorkspaceInitials,
  persistWorkspaceInitials,
} from "./workspaceCardMetadata";

interface WorkspaceCardGridProps {
  workspaces: Workspace[];
  selectedWorkspaceId: string | null;
  statuses: Record<string, TabStatus | null>;
  sortIds: string[];
  onSelect: (workspaceId: string) => void;
  onCreate: () => void;
}

const CARD_WIDTH = 42;
const CARD_GAP = 4;
const EDITOR_WIDTH = 190;
const EDITOR_HEIGHT = 82;

const COLLAPSED_ROWS = 2;

/**
 * Collapsed, two rows hold the cards plus one trailing control (create, or "+N" when
 * more are hidden); the selected workspace always stays visible. Expanded shows all.
 */
export function visibleWorkspaceIds(
  ids: readonly string[],
  selectedId: string | null,
  columns: number,
  expanded: boolean,
): { visible: string[]; hidden: number } {
  const capacity = Math.max(1, columns * COLLAPSED_ROWS - 1);
  if (expanded || ids.length <= capacity) return { visible: [...ids], hidden: 0 };
  const visible = ids.slice(0, capacity);
  if (selectedId && ids.includes(selectedId) && !visible.includes(selectedId)) visible[capacity - 1] = selectedId;
  return { visible, hidden: ids.length - visible.length };
}

/** Puts a reordered visible subset back into the full order; hidden ids keep their slots. */
export function mergeVisibleOrder(all: readonly string[], visibleOrdered: readonly string[]): string[] {
  const visibleSet = new Set(visibleOrdered);
  let next = 0;
  return all.map((id) => (visibleSet.has(id) ? visibleOrdered[next++] : id));
}

export function WorkspaceCardGrid({ workspaces, selectedWorkspaceId, statuses, sortIds, onSelect, onCreate }: WorkspaceCardGridProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const editorRef = useRef<HTMLDivElement>(null);
  const [columns, setColumns] = useState(3);
  const [customInitials, setCustomInitials] = useState(loadWorkspaceInitials);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editorPosition, setEditorPosition] = useState({ left: 0, top: 0 });
  const [draft, setDraft] = useState("");
  const [expanded, setExpanded] = useState(false);
  const allIds = workspaces.map((workspace) => workspace.id);
  const { visible, hidden } = visibleWorkspaceIds(allIds, selectedWorkspaceId, columns, expanded);
  const visibleSet = new Set(visible);
  const shown = visible.map((id) => workspaces.find((workspace) => workspace.id === id)).filter((workspace): workspace is Workspace => workspace != null);
  const visibleSortIds = sortIds.filter((sortId) => visibleSet.has(sortId.slice(WS_PREFIX.length)));
  const overflow = expanded || hidden > 0;
  const fullOrder = (ordered: string[]) => mergeVisibleOrder(allIds, ordered);

  const beginEditing = useCallback((workspace: Workspace, anchor: HTMLElement) => {
    const rect = anchor.closest<HTMLElement>('[data-testid="workspace-card"]')?.getBoundingClientRect()
      ?? anchor.getBoundingClientRect();
    const right = rect.right + 6;
    const left = right + EDITOR_WIDTH <= window.innerWidth - 8
      ? right
      : Math.max(8, rect.left - EDITOR_WIDTH - 6);
    const top = Math.max(8, Math.min(rect.top, window.innerHeight - EDITOR_HEIGHT - 8));
    setDraft(customInitials[workspace.id] ?? defaultWorkspaceInitials(workspace.name));
    setEditorPosition({ left, top });
    setEditingId(workspace.id);
  }, [customInitials]);

  const closeEditor = useCallback((save: boolean) => {
    if (save && editingId) {
      const initials = normalizeWorkspaceInitials(draft);
      if (initials) {
        setCustomInitials((current) => {
          const next = { ...current, [editingId]: initials };
          persistWorkspaceInitials(next);
          return next;
        });
      }
    }
    setEditingId(null);
  }, [draft, editingId]);

  useEffect(() => {
    if (!editingId) return;
    const closeOutside = (event: PointerEvent) => {
      if (!editorRef.current?.contains(event.target as Node)) closeEditor(true);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") closeEditor(false);
    };
    document.addEventListener("pointerdown", closeOutside, true);
    document.addEventListener("keydown", closeOnEscape, true);
    return () => {
      document.removeEventListener("pointerdown", closeOutside, true);
      document.removeEventListener("keydown", closeOnEscape, true);
    };
  }, [closeEditor, editingId]);

  useLayoutEffect(() => {
    const host = scrollRef.current;
    if (!host) return;
    const updateColumns = () => {
      if (host.clientWidth > 0) setColumns(Math.max(1, Math.floor((host.clientWidth - 16 + CARD_GAP) / (CARD_WIDTH + CARD_GAP))));
    };
    updateColumns();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(updateColumns);
    observer.observe(host);
    return () => observer.disconnect();
  }, []);

  const editingWorkspace = workspaces.find((workspace) => workspace.id === editingId) ?? null;

  return <>
    <div ref={scrollRef} className="sidebar-workspace-card-scroll" data-testid="workspace-card-scroll" role="region" aria-label="Workspace cards">
      <SortableContext items={visibleSortIds} strategy={rectSortingStrategy}>
        <div
          className="sidebar-workspace-card-grid"
          data-testid="workspace-card-grid"
          data-expanded={expanded || undefined}
          role="list"
          aria-label="Workspaces"
          style={{ gridTemplateColumns: `repeat(${columns}, 42px)` }}
        >
          {shown.map((workspace) => {
            const selected = workspace.id === selectedWorkspaceId;
            const status = statuses[workspace.id];
            return (
              <SortableRow
                key={workspace.id}
                id={`${WS_PREFIX}${workspace.id}`}
                data={{ group: "workspace", groupKey: "workspaces", fullOrder }}
              >
                <div
                  className="sidebar-workspace-card"
                  data-testid="workspace-card"
                  data-workspace-id={workspace.id}
                  data-selected={selected ? "true" : "false"}
                  data-status={status?.toLowerCase() ?? "none"}
                  role="listitem"
                >
                  <button
                    type="button"
                    className="sidebar-workspace-card-select"
                    aria-label={`Select workspace ${workspace.name}`}
                    aria-pressed={selected}
                    aria-current={selected ? "true" : undefined}
                    title={workspace.name}
                    onClick={() => onSelect(workspace.id)}
                    onDoubleClick={(event) => beginEditing(workspace, event.currentTarget)}
                  >
                    <span className="sidebar-workspace-card-initials">
                      {customInitials[workspace.id] ?? defaultWorkspaceInitials(workspace.name)}
                    </span>
                  </button>
                  {selected && (
                    <button
                      type="button"
                      className="sidebar-workspace-card-edit"
                      aria-label={`Edit initials for ${workspace.name}`}
                      title="Edit initials"
                      onPointerDown={(event) => event.stopPropagation()}
                      onClick={(event) => beginEditing(workspace, event.currentTarget)}
                    >
                      <Pencil size={9} />
                    </button>
                  )}
                  {status === "Working" && !selected && (
                    <span className="sidebar-workspace-card-working" role="status" aria-label={`${workspace.name} has working tabs`}>
                      <ActivityDots size="micro" tone="on-accent" variant="workspace-badge" active />
                    </span>
                  )}
                </div>
              </SortableRow>
            );
          })}
          {hidden > 0 ? (
            <button
              type="button"
              data-testid="workspace-card-more"
              className="sidebar-workspace-card-create sidebar-workspace-card-more"
              aria-label={`Show all workspaces, ${hidden} more`}
              aria-expanded={false}
              title="Show all workspaces"
              onClick={() => setExpanded(true)}
            >
              +{hidden}
            </button>
          ) : (
            <button
              type="button"
              data-testid="workspace-card-create"
              className="sidebar-workspace-card-create"
              aria-label="Create workspace"
              title="New workspace"
              onClick={onCreate}
            >
              <Plus size={17} strokeWidth={1.5} />
            </button>
          )}
          {expanded && overflow && (
            <button
              type="button"
              data-testid="workspace-card-less"
              className="sidebar-workspace-card-create sidebar-workspace-card-more"
              aria-label="Show fewer workspaces"
              aria-expanded={true}
              title="Show fewer"
              onClick={() => setExpanded(false)}
            >
              <ChevronUp size={16} strokeWidth={1.75} />
            </button>
          )}
        </div>
      </SortableContext>
    </div>
    {editingWorkspace && createPortal(
      <div
        ref={editorRef}
        role="dialog"
        aria-label={`Edit ${editingWorkspace.name} workspace card`}
        className="sidebar-workspace-card-popover ct-float"
        style={{ left: editorPosition.left, top: editorPosition.top }}
      >
        <label className="sidebar-workspace-card-popover-label">
          Initials
          <input
            autoFocus
            aria-label={`Initials for ${editingWorkspace.name}`}
            className="sidebar-workspace-card-popover-input"
            value={draft}
            maxLength={2}
            onChange={(event) => setDraft(normalizeWorkspaceInitials(event.target.value))}
            onKeyDown={(event) => {
              if (event.key === "Enter") closeEditor(true);
            }}
          />
        </label>
        <button type="button" className="sidebar-workspace-card-popover-done" onClick={() => closeEditor(true)}>Done</button>
      </div>,
      document.body,
    )}
  </>;
}
