import { useCallback, useRef, useState } from "react";
import { createNoticePublisher } from "../../lib/notices";
import { defaultSpawnProviderId } from "../../lib/agentDescriptorCache";
import { isFreeLimitRefusal } from "../billing/freeLimitModel";
import { isWorkspaceCwdRefusal, spawnErrorMessage } from "../../lib/spawnErrorMessage";
import { openNewSessionPicker } from "../../lib/newSessionChoice";
import { useWorkspaceBrowseTarget } from "../../lib/workspaceBrowseTarget";
import { usePairedWorkspaceInventory } from "../../store/multiplexer/context";
import { useMultiplexer } from "../../store/multiplexer/hooks";
import { remotePeerActionEligibility } from "./remoteSidebarActionsModel";
import { SidebarWorkspaceTabs, type SidebarWorkspaceTabsProps } from "./SidebarWorkspaceTabs";
import { useSidebarMachineFilter } from "./sidebarMachineFilter";

interface GeneralWorkspaceTabsProps extends Omit<
  SidebarWorkspaceTabsProps,
  "onNewSession" | "newSessionTitle" | "newSessionDisabledReason"
> {
  providerId?: string;
  generalCwd: string | null;
}

export function GeneralWorkspaceTabs({
  providerId,
  generalCwd,
  newSessionDisabled,
  ...props
}: GeneralWorkspaceTabsProps) {
  const machineFilter = useSidebarMachineFilter();
  const browseTarget = useWorkspaceBrowseTarget();
  const { generals, refreshPeerInventory } = usePairedWorkspaceInventory();
  const { spawnAgentTab, upsertToast } = useMultiplexer();
  const [spawningRemote, setSpawningRemote] = useState(false);
  const spawningRemoteRef = useRef(false);

  const remotePeerId = typeof machineFilter === "object" ? machineFilter.peerId : null;
  const remotePeer = remotePeerId ? generals.find((peer) => peer.peerId === remotePeerId) : undefined;
  const remotePeerName = remotePeer?.name ?? (browseTarget?.id === remotePeerId ? browseTarget.name : undefined) ?? "Machine";
  const remoteEligibility = remotePeerId ? remotePeerActionEligibility(remotePeer?.availability) : null;

  const createOnRemotePeer = useCallback(async () => {
    if (!remotePeerId || !remoteEligibility?.allowed || newSessionDisabled || spawningRemoteRef.current) return;
    spawningRemoteRef.current = true;
    setSpawningRemote(true);
    try {
      const created = await spawnAgentTab(
        props.workspace.id,
        providerId ?? defaultSpawnProviderId(),
        undefined,
        undefined,
        undefined,
        undefined,
        "fresh_general",
        undefined,
        undefined,
        undefined,
        remotePeerId,
      );
      if (created) refreshPeerInventory?.(remotePeerId);
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
    } finally {
      spawningRemoteRef.current = false;
      setSpawningRemote(false);
    }
  }, [
    newSessionDisabled,
    props.workspace.id,
    providerId,
    refreshPeerInventory,
    remoteEligibility?.allowed,
    remotePeerId,
    spawnAgentTab,
    upsertToast,
  ]);

  const effectiveDisabled = remotePeerId
    ? Boolean(newSessionDisabled) || !remoteEligibility?.allowed || spawningRemote
    : Boolean(newSessionDisabled) || !generalCwd;

  const newSessionDisabledReason = remotePeerId && !newSessionDisabled && remoteEligibility && !remoteEligibility.allowed
    ? `${remotePeerName} is ${remoteEligibility.reason}`
    : undefined;

  const newSessionTitle = remotePeerId
    ? remoteEligibility && !remoteEligibility.allowed
      ? `New session unavailable while ${remotePeerName} is ${remoteEligibility.reason}`
      : `New session on ${remotePeerName}`
    : undefined;

  return <SidebarWorkspaceTabs
    {...props}
    newSessionDisabled={effectiveDisabled}
    newSessionTitle={newSessionTitle}
    newSessionDisabledReason={newSessionDisabledReason}
    onNewSession={() => {
      if (remotePeerId) {
        void createOnRemotePeer();
        return;
      }
      if (newSessionDisabled || !generalCwd) return;
      openNewSessionPicker({ workspaceId: props.workspace.id, workspaceCwd: generalCwd, initialProviderId: providerId, section: "general" });
    }}
  />;
}
