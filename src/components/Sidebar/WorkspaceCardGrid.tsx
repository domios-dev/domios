import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { Pencil } from "lucide-react";
import type { PaneStatus, Workspace } from "../../lib/types";
import { SortableContext, SortableRow, WS_PREFIX, rectSortingStrategy } from "./sortable";
import {
  defaultWorkspaceInitials,
  loadWorkspaceInitials,
  normalizeWorkspaceInitials,
  persistWorkspaceInitials,
  workspaceCardColor,
} from "./workspaceCardMetadata";

interface WorkspaceCardGridProps {
  workspaces: Workspace[];
  selectedWorkspaceId: string | null;
  statuses: Record<string, PaneStatus | null>;
  sortIds: string[];
  onSelect: (workspaceId: string) => void;
}

const CARD_WIDTH = 52;
const CARD_GAP = 5;

export function workspaceCardPosition(index: number, columns: number): { gridColumn: number; gridRow: number } {
  const pageSize = columns * 2;
  const page = Math.floor(index / pageSize);
  const pageIndex = index % pageSize;
  return {
    gridColumn: page * columns + (pageIndex % columns) + 1,
    gridRow: Math.floor(pageIndex / columns) + 1,
  };
}

export function WorkspaceCardGrid({ workspaces, selectedWorkspaceId, statuses, sortIds, onSelect }: WorkspaceCardGridProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const [columns, setColumns] = useState(3);
  const [customInitials, setCustomInitials] = useState(loadWorkspaceInitials);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState("");

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
        <div className="sidebar-workspace-card-grid" data-testid="workspace-card-grid" role="list" aria-label="Workspaces">
          {workspaces.map((workspace, index) => {
            const selected = workspace.id === selectedWorkspaceId;
            const color = workspaceCardColor(workspace.id);
            const status = statuses[workspace.id];
            return (
              <SortableRow
                key={workspace.id}
                id={`${WS_PREFIX}${workspace.id}`}
                data={{ group: "workspace", groupKey: "workspaces", items: sortIds }}
              >
                <div
                  className="sidebar-workspace-card"
                  data-testid="workspace-card"
                  data-workspace-id={workspace.id}
                  data-selected={selected ? "true" : "false"}
                  data-status={status?.toLowerCase() ?? "none"}
                  role="listitem"
                  style={{ ["--workspace-card-color" as string]: color, ...workspaceCardPosition(index, columns) }}
                >
                  {editingId === workspace.id ? (
                    <input
                      autoFocus
                      aria-label={`Initials for ${workspace.name}`}
                      className="sidebar-workspace-card-initials-input"
                      value={draft}
                      maxLength={2}
                      onPointerDown={(event) => event.stopPropagation()}
                      onChange={(event) => setDraft(normalizeWorkspaceInitials(event.target.value))}
                      onBlur={() => commit(workspace)}
                      onKeyDown={(event) => {
                        if (event.key === "Enter") event.currentTarget.blur();
                        if (event.key === "Escape") setEditingId(null);
                      }}
                    />
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
        </div>
      </SortableContext>
    </div>
  );
}
