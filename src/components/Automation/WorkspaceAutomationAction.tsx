import { Workflow } from "lucide-react";

export function WorkspaceAutomationAction({ workspaceId, workspaceName, onOpen, touchMode = false }: { workspaceId: string; workspaceName: string; onOpen: (workspaceId: string) => void; touchMode?: boolean }) {
  return <button type="button" data-testid="workspace-automation-action" aria-label={`Open automations for ${workspaceName}`} className={`${touchMode ? "h-11 w-11" : "w-5 h-5"} flex items-center justify-center rounded text-[var(--ct-muted-fg)] hover:bg-[var(--ct-border)] focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]`} onClick={(event) => { event.stopPropagation(); onOpen(workspaceId); }}><Workflow size={touchMode ? 17 : 13} aria-hidden="true" /></button>;
}
