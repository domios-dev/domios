import { openNewSessionPicker } from "../../lib/newSessionChoice";
import { SidebarWorkspaceTabs, type SidebarWorkspaceTabsProps } from "./SidebarWorkspaceTabs";

interface GeneralWorkspaceTabsProps extends Omit<SidebarWorkspaceTabsProps, "onNewSession"> {
  providerId?: string;
  generalCwd: string | null;
}

export function GeneralWorkspaceTabs({
  providerId,
  generalCwd,
  newSessionDisabled,
  ...props
}: GeneralWorkspaceTabsProps) {
  return <SidebarWorkspaceTabs
    {...props}
    newSessionDisabled={newSessionDisabled || !generalCwd}
    onNewSession={() => {
      if (newSessionDisabled || !generalCwd) return;
      openNewSessionPicker({ workspaceId: props.workspace.id, workspaceCwd: generalCwd, initialProviderId: providerId, section: "general" });
    }}
  />;
}
