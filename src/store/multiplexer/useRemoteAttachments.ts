import { useCallback, useEffect, type Dispatch, type MutableRefObject } from "react";
import { invoke, listen } from "../../lib/bridge";
import { DEV_BUILD } from "../../lib/devBuild";
import { projectAttachedRemoteTab, saveAttachedRemoteTabs, type RemotePeerTabs } from "../../lib/remoteTabPortal";
import { managersToAttach, TEMPLATE_PEER_OUTCOME_EVENT, type TemplatePeerStartOutcome } from "../../lib/teamTemplates/peerStartOutcome";
import type { Tab, TabStatus } from "../../lib/types";
import { parseCompositeTabId } from "../../lib/remoteTabTransport";
import { remoteChatPeer } from "../../lib/remoteChatStatus";
import type { Action } from "./reducer";
import { remoteTabsFromOrdinaryProjection } from "./remoteTabProjection";
import type { ExtendedState } from "./tree";

export function useRemoteAttachments({ attachedRef, stateRef, dispatch, applyRemoteProjection }: {
  attachedRef: MutableRefObject<Record<string, Tab>>;
  stateRef: MutableRefObject<ExtendedState>;
  dispatch: Dispatch<Action>;
  applyRemoteProjection: (tabs: Record<string, Tab>, remoteStatuses?: Record<string, TabStatus | null>) => void;
}) {
  const remember = useCallback((next: Record<string, Tab>) => {
    attachedRef.current = next;
    saveAttachedRemoteTabs(next);
  }, [attachedRef]);

  const attachRemoteTab = useCallback((workspaceId: string, peer: RemotePeerTabs, tab: Tab, status?: TabStatus) => {
    if (tab.remote_peer_id || tab.remote_tab_id || tab.id.startsWith("mesh:")
      || !parseCompositeTabId(`mesh:${peer.peer_id}/${tab.id}`)
      || remoteChatPeer(peer.peer_id)?.presence === "not-paired") return;
    const projected = projectAttachedRemoteTab(tab, peer, workspaceId);
    remember({ ...attachedRef.current, [projected.id]: projected });
    applyRemoteProjection(remoteTabsFromOrdinaryProjection(Object.values(stateRef.current.tabs)),
      status === undefined ? undefined : { [projected.id]: status });
    dispatch({ type: "SET_ACTIVE_WORKSPACE", source: "program", id: workspaceId });
    dispatch({ type: "SET_ACTIVE_TAB", source: "program", workspaceId, tabId: projected.id });
    dispatch({ type: "SET_FOCUSED_TAB", tabId: projected.id });
  }, [applyRemoteProjection, attachedRef, dispatch, remember, stateRef]);

  // A Team started on a peer shows its managers in the workspace its form was opened from.
  useEffect(() => {
    let active = true;
    let stop: (() => void) | undefined;
    void listen<TemplatePeerStartOutcome>(TEMPLATE_PEER_OUTCOME_EVENT, ({ payload }) => {
      if (payload.state !== "started") return;
      const workspaceId = payload.origin_workspace_id ?? stateRef.current.active_workspace_id;
      if (!workspaceId) return;
      void invoke<RemotePeerTabs>("mesh_peer_tabs_list", { peerId: payload.peer_id }).then((peer) => {
        if (!active) return;
        for (const tab of managersToAttach(payload, peer.tabs)) attachRemoteTab(workspaceId, peer, tab, peer.tab_statuses?.[tab.id]);
      }, (error: unknown) => {
        // The outcome itself is already a system notice; only the attach failed.
        if (DEV_BUILD) console.debug("[peer-team] managers could not be listed:", error);
      });
    }).then((unlisten) => {
      if (active) stop = unlisten;
      else unlisten();
    }, () => undefined);
    return () => { active = false; stop?.(); };
  }, [attachRemoteTab, stateRef]);

  return attachRemoteTab;
}
