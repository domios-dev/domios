import { Workflow } from "lucide-react";

interface WorkspaceAutomationActionProps {
  workspaceId: string;
  workspaceName: string;
  onOpen: (workspaceId: string) => void;
  touchMode?: boolean;
  revealOnHover?: boolean;
}

export function WorkspaceAutomationAction({
  workspaceId,
  workspaceName,
  onOpen,
  touchMode = false,
  revealOnHover = false,
}: WorkspaceAutomationActionProps) {
  const visibilityClasses = touchMode
    ? "h-11 w-11"
    : `w-5 h-5 ${revealOnHover ? "opacity-0 group-hover:opacity-60 focus-visible:opacity-100 hover:!opacity-100" : ""}`;
  const transitionClasses = revealOnHover ? "transition-opacity" : "";

  return (
    <button
      type="button"
      data-testid="workspace-automation-action"
      aria-label={`Open jobs for ${workspaceName}`}
      title="Jobs"
      className={`${visibilityClasses} flex items-center justify-center rounded text-[var(--ct-muted-fg)] ${transitionClasses} hover:bg-[var(--ct-border)] focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]`}
      onClick={(event) => {
        event.stopPropagation();
        onOpen(workspaceId);
      }}
    >
      <Workflow size={touchMode ? 17 : 13} aria-hidden="true" />
    </button>
  );
}
