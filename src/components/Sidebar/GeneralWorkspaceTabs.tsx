import { useMultiplexer } from "../../store/multiplexer";
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
  const { spawnAgentTab } = useMultiplexer();
  return <SidebarWorkspaceTabs
    {...props}
    newSessionDisabled={newSessionDisabled || !providerId || !generalCwd}
    onNewSession={() => {
      if (newSessionDisabled || !providerId || !generalCwd) return;
      void spawnAgentTab(props.workspace.id, providerId, generalCwd, undefined, undefined, undefined, "fresh_general");
    }}
  />;
}
