import type { HTMLAttributes } from "react";
import "./workspace-cube-grid.css";

export interface WorkspaceCube {
  id: string;
  initials: string;
  name: string;
  selected?: boolean;
  working?: boolean;
}

export interface WorkspaceCubeGridProps extends HTMLAttributes<HTMLDivElement> {
  workspaces: WorkspaceCube[];
  onSelect?: (id: string) => void;
  onCreate?: () => void;
  createLabel?: string;
}

export function WorkspaceCubeGrid({ workspaces, onSelect, onCreate, createLabel = "Create workspace", className = "", ...props }: WorkspaceCubeGridProps) {
  return <div className={`ds-root ds-workspace-grid ${className}`} role="list" aria-label="Workspaces" {...props}>
    {workspaces.map((workspace) => <div key={workspace.id} className={`ds-workspace-cube${workspace.selected ? " ds-workspace-cube--selected" : ""}`} role="listitem">
      <button type="button" className="ds-workspace-cube__select" aria-label={`Select workspace ${workspace.name}`} aria-pressed={workspace.selected} onClick={() => onSelect?.(workspace.id)}>
        <span>{workspace.initials}</span>
      </button>
      {workspace.working && <span className="ds-workspace-cube__dots" role="status" aria-label={`${workspace.name} has working tabs`}><i /><i /><i /></span>}
    </div>)}
    {onCreate && <button type="button" className="ds-workspace-cube__create" aria-label={createLabel} onClick={onCreate}>+</button>}
  </div>;
}
