import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { Palette, Pencil, Plus } from "lucide-react";
import type { PaneStatus, Workspace } from "../../lib/types";
import { SortableContext, SortableRow, WS_PREFIX, rectSortingStrategy } from "./sortable";
import {
  defaultWorkspaceInitials,
  loadWorkspaceColorIndexes,
  loadWorkspaceInitials,
  normalizeWorkspaceInitials,
  persistWorkspaceInitials,
  persistWorkspaceColorIndexes,
  WORKSPACE_CARD_COLORS,
  workspaceCardColor,
  workspaceCardColorIndex,
} from "./workspaceCardMetadata";

interface WorkspaceCardGridProps {
  workspaces: Workspace[];
  selectedWorkspaceId: string | null;
  statuses: Record<string, PaneStatus | null>;
  sortIds: string[];
  onSelect: (workspaceId: string) => void;
  onCreate: () => void;
}

const CARD_WIDTH = 42;
const CARD_GAP = 4;

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
  const [columns, setColumns] = useState(3);
  const [customInitials, setCustomInitials] = useState(loadWorkspaceInitials);
  const [customColors, setCustomColors] = useState(loadWorkspaceColorIndexes);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const rows = Math.min(2, Math.ceil((workspaces.length + 1) / columns));

  const beginEditing = useCallback((workspace: Workspace) => {
    setDraft(customInitials[workspace.id] ?? defaultWorkspaceInitials(workspace.name));
    setEditingId(workspace.id);
  }, [customInitials]);

  const commit = useCallback((workspace: Workspace) => {
    const initials = normalizeWorkspaceInitials(draft);
    setEditingId(null);
    if (!initials) return;
    setCustomInitials((current) => {
      const next = { ...current, [workspace.id]: initials };
      persistWorkspaceInitials(next);
      return next;
    });
  }, [draft]);

  const cycleColor = useCallback((workspace: Workspace) => {
    setCustomColors((current) => {
      const activeIndex = current[workspace.id] ?? workspaceCardColorIndex(workspace.id);
      const next = { ...current, [workspace.id]: (activeIndex + 1) % WORKSPACE_CARD_COLORS.length };
      persistWorkspaceColorIndexes(next);
      return next;
    });
  }, []);

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

  return (
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
            const color = workspaceCardColor(workspace.id, customColors[workspace.id]);
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
                  style={{ ["--workspace-card-color" as string]: color }}
                >
                  {editingId === workspace.id ? (
                    <div
                      className="sidebar-workspace-card-editor"
                      onBlur={(event) => {
                        if (!event.currentTarget.contains(event.relatedTarget as Node)) commit(workspace);
                      }}
                    >
                      <input
                        autoFocus
                        aria-label={`Initials for ${workspace.name}`}
                        className="sidebar-workspace-card-initials-input"
                        value={draft}
                        maxLength={2}
                        onPointerDown={(event) => event.stopPropagation()}
                        onChange={(event) => setDraft(normalizeWorkspaceInitials(event.target.value))}
                        onKeyDown={(event) => {
                          if (event.key === "Enter") event.currentTarget.blur();
                          if (event.key === "Escape") setEditingId(null);
                        }}
                      />
                      <button
                        type="button"
                        className="sidebar-workspace-card-color-edit"
                        aria-label={`Change color for ${workspace.name}`}
                        title="Change color"
                        onPointerDown={(event) => {
                          event.preventDefault();
                          event.stopPropagation();
                        }}
                        onClick={() => cycleColor(workspace)}
                      >
                        <Palette size={8} />
                      </button>
                    </div>
                  ) : (
                    <button
                      type="button"
                      className="sidebar-workspace-card-select"
                      aria-label={`Select workspace ${workspace.name}`}
                      aria-pressed={selected}
                      aria-current={selected ? "true" : undefined}
                      title={workspace.name}
                      onClick={() => onSelect(workspace.id)}
                      onDoubleClick={() => beginEditing(workspace)}
                    >
                      <span className="sidebar-workspace-card-initials">
                        {customInitials[workspace.id] ?? defaultWorkspaceInitials(workspace.name)}
                      </span>
                    </button>
                  )}
                  {selected && editingId !== workspace.id && (
                    <button
                      type="button"
                      className="sidebar-workspace-card-edit"
                      aria-label={`Edit initials for ${workspace.name}`}
                      title="Edit initials"
                      onPointerDown={(event) => event.stopPropagation()}
                      onClick={() => beginEditing(workspace)}
                    >
                      <Pencil size={9} />
                    </button>
                  )}
                  {status === "Working" && <span className="sidebar-workspace-card-working" role="status" aria-label={`${workspace.name} has working tabs`} />}
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
  );
}
