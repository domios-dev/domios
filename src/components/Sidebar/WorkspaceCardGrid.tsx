import { useCallback, useState } from "react";
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

export function WorkspaceCardGrid({ workspaces, selectedWorkspaceId, statuses, sortIds, onSelect }: WorkspaceCardGridProps) {
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

  return (
    <div className="sidebar-workspace-card-scroll" data-testid="workspace-card-scroll">
      <SortableContext items={sortIds} strategy={rectSortingStrategy}>
        <div className="sidebar-workspace-card-grid" data-testid="workspace-card-grid">
          {workspaces.map((workspace) => {
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
                  style={{ ["--workspace-card-color" as string]: color }}
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
                      title={workspace.name}
                      onClick={() => onSelect(workspace.id)}
                      onDoubleClick={() => beginEditing(workspace)}
                    >
                      <span className="sidebar-workspace-card-initials">
                        {customInitials[workspace.id] ?? defaultWorkspaceInitials(workspace.name)}
                      </span>
                      <span className="sidebar-workspace-card-name">{workspace.name}</span>
                      <span className="sidebar-workspace-card-count">{workspace.tabs.length}</span>
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
                  {status === "Working" && <span className="sidebar-workspace-card-working" aria-label="Workspace has working tabs" />}
                </div>
              </SortableRow>
            );
          })}
        </div>
      </SortableContext>
    </div>
  );
}
