import { useCallback } from "react";
import { MonitorUp } from "lucide-react";
import { createNoticePublisher } from "../../lib/notices";
import { defaultSpawnProviderId } from "../../lib/agentDescriptorCache";
import { isFreeLimitRefusal } from "../billing/freeLimitModel";
import { isWorkspaceCwdRefusal, spawnErrorMessage } from "../../lib/spawnErrorMessage";
import { openNewSessionPicker } from "../../lib/newSessionChoice";
import { LOCAL_MACHINE_ID } from "../../lib/machinePlacement";
import { useMachinePlacementInventory } from "../shared/useMachinePlacementInventory";
import { MachinePlacementMenu } from "../shared/MachinePlacement";
import { useMultiplexer } from "../../store/multiplexer/hooks";
import { SidebarWorkspaceTabs, type SidebarWorkspaceTabsProps } from "./SidebarWorkspaceTabs";
import { setSidebarMachineFilter } from "./sidebarMachineFilter";

interface GeneralWorkspaceTabsProps extends Omit<SidebarWorkspaceTabsProps, "onNewSession" | "onOpenRemoteTab" | "newGeneralSessionMenu"> {
  providerId?: string;
  generalCwd: string | null;
}

export function GeneralWorkspaceTabs({
  providerId,
  generalCwd,
  newSessionDisabled,
  ...props
}: GeneralWorkspaceTabsProps) {
  const inventory = useMachinePlacementInventory();
  const { spawnAgentTab, selectTabByTab, upsertToast } = useMultiplexer();
  const createOnMachine = useCallback(async (machineId: string) => {
    const machine = inventory.choices?.find((choice) => choice.id === machineId);
    if (!machine || machine.unavailableReason || newSessionDisabled || (machine.local && !generalCwd)) return;
    try {
      const created = await spawnAgentTab(
        props.workspace.id,
        providerId ?? defaultSpawnProviderId(),
        machine.local ? generalCwd ?? undefined : undefined,
        undefined,
        undefined,
        undefined,
        "fresh_general",
        undefined,
        undefined,
        undefined,
        machine.local ? undefined : machine.id,
        undefined,
        machine.local ? undefined : () => setSidebarMachineFilter("all"),
      );
      if (created && machine.local) await selectTabByTab(created.workspaceId, created.tabId).catch(() => {});
    } catch (error) {
      if (isWorkspaceCwdRefusal(error) || isFreeLimitRefusal(error)) return;
      createNoticePublisher({ upsertToast })({
        kind: "toast",
        id: `spawn:${props.workspace.id}`,
        tabId: null,
        workspaceId: props.workspace.id,
        title: "Session start failed",
        detail: spawnErrorMessage(error),
        severity: "Error",
      });
    }
  }, [generalCwd, inventory.choices, newSessionDisabled, props.workspace.id, providerId, selectTabByTab, spawnAgentTab, upsertToast]);

  const machineMenu = <MachinePlacementMenu
    choices={inventory.choices}
    value={LOCAL_MACHINE_ID}
    onChange={(machineId) => { void createOnMachine(machineId); }}
    menuLabel="New General session on"
    header={<div className="ct-menu-head"><div className="truncate px-2 pt-1.5 pb-1 text-[13px] font-semibold text-[var(--ct-fg)]">New General session on</div></div>}
    disabled={newSessionDisabled}
    disabledLocal={!generalCwd}
    testId="new-general-session-machine"
    trigger={<button type="button" aria-label="New session on another machine" title="New session on another machine"
      disabled={newSessionDisabled}
      className={`flex shrink-0 items-center justify-center rounded-md text-[var(--ct-muted)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-muted-fg)] disabled:opacity-40 ${props.touchMode ? "h-11 w-11" : "h-6 w-6"}`}>
      <MonitorUp size={12} strokeWidth={1.5} />
    </button>}
  />;

  return <SidebarWorkspaceTabs
    {...props}
    newSessionDisabled={newSessionDisabled || !generalCwd}
    newGeneralSessionMenu={machineMenu}
    onNewSession={() => {
      if (newSessionDisabled || !generalCwd) return;
      openNewSessionPicker({ workspaceId: props.workspace.id, workspaceCwd: generalCwd, initialProviderId: providerId, section: "general" });
    }}
  />;
}
