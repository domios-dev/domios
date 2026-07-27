import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Pencil, Plus } from "lucide-react";
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

export function workspaceCardPosition(index: number, columns: number): { gridColumn: number; gridRow: number } {
  const pageSize = columns * 2;
  const page = Math.floor(index / pageSize);
  const pageIndex = index % pageSize;
  return {
    gridColumn: page * columns + (pageIndex % columns) + 1,
    gridRow: Math.floor(pageIndex / columns) + 1,
  };
}

export function WorkspaceCardGrid({ workspaces, selectedWorkspaceId, statuses, sortIds, onSelect, onCreate }: WorkspaceCardGridProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const editorRef = useRef<HTMLDivElement>(null);
  const [columns, setColumns] = useState(3);
  const [customInitials, setCustomInitials] = useState(loadWorkspaceInitials);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editorPosition, setEditorPosition] = useState({ left: 0, top: 0 });
  const [draft, setDraft] = useState("");
  const rows = Math.min(2, Math.ceil((workspaces.length + 1) / columns));

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
      <SortableContext items={sortIds} strategy={rectSortingStrategy}>
        <div
          className="sidebar-workspace-card-grid"
          data-testid="workspace-card-grid"
          role="list"
          aria-label="Workspaces"
          style={{ gridTemplateRows: `repeat(${rows}, 40px)` }}
        >
          {workspaces.map((workspace, index) => {
            const selected = workspace.id === selectedWorkspaceId;
            const status = statuses[workspace.id];
            return (
              <SortableRow
                key={workspace.id}
                id={`${WS_PREFIX}${workspace.id}`}
                data={{ group: "workspace", groupKey: "workspaces", items: sortIds }}
                layoutStyle={workspaceCardPosition(index, columns)}
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
                      <ActivityDots size="micro" tone="muted" />
                    </span>
                  )}
                </div>
              </SortableRow>
            );
          })}
          <button
            type="button"
            data-testid="workspace-card-create"
            className="sidebar-workspace-card-create"
            aria-label="Create workspace"
            title="New workspace"
            onClick={onCreate}
            style={workspaceCardPosition(workspaces.length, columns)}
          >
            <Plus size={17} strokeWidth={1.5} />
          </button>
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
