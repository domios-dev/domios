import { useCallback, useEffect, type Dispatch, type MutableRefObject } from "react";
import { invoke, listen } from "../../lib/bridge";
import { DEV_BUILD } from "../../lib/devBuild";
import { projectAttachedRemoteTab, saveAttachedRemoteTabs, type RemotePeerTabs } from "../../lib/remoteTabPortal";
import { managersToAttach, TEMPLATE_PEER_OUTCOME_EVENT, type TemplatePeerStartOutcome } from "../../lib/teamTemplates/peerStartOutcome";
import type { Tab } from "../../lib/types";
import type { Action } from "./reducer";
import { remoteTabsFromOrdinaryProjection } from "./remoteTabProjection";
import type { ExtendedState } from "./tree";

/**
 * Remote tabs this machine shows on purpose: attached from the picker or from a Team started on a peer, and detached
 * again here without touching the peer.
 */
export function useRemoteAttachments({ attachedRef, stateRef, dispatch, applyRemoteProjection }: {
  attachedRef: MutableRefObject<Record<string, Tab>>;
  stateRef: MutableRefObject<ExtendedState>;
  dispatch: Dispatch<Action>;
  applyRemoteProjection: (tabs: Record<string, Tab>) => void;
}) {
  const remember = useCallback((next: Record<string, Tab>) => {
    attachedRef.current = next;
    saveAttachedRemoteTabs(next);
  }, [attachedRef]);

  const attachRemoteTab = useCallback((workspaceId: string, peer: RemotePeerTabs, tab: Tab) => {
    const projected = projectAttachedRemoteTab(tab, peer, workspaceId);
    remember({ ...attachedRef.current, [projected.id]: projected });
    applyRemoteProjection(remoteTabsFromOrdinaryProjection(Object.values(stateRef.current.tabs)));
    dispatch({ type: "SET_ACTIVE_WORKSPACE", source: "program", id: workspaceId });
    dispatch({ type: "SET_ACTIVE_TAB", source: "program", workspaceId, tabId: projected.id });
    dispatch({ type: "SET_FOCUSED_TAB", tabId: projected.id });
  }, [applyRemoteProjection, attachedRef, dispatch, remember, stateRef]);

  /** Removes only this machine's attachment; the tab keeps running on its peer. */
  const detachRemoteTab = useCallback((tabId: string) => {
    const attachment = attachedRef.current[tabId];
    if (!attachment) return;
    const { [tabId]: _detached, ...remaining } = attachedRef.current;
    remember(remaining);
    const workspaceId = stateRef.current.tabs[tabId]?.workspace_id ?? attachment.workspace_id;
    if (workspaceId) dispatch({ type: "CLOSE_TAB", workspaceId, tabId, removedTabIds: [tabId] });
    dispatch({ type: "REMOVE_PENDING_CLOSE", tabIds: [tabId] });
    const { [tabId]: _shown, ...others } = remoteTabsFromOrdinaryProjection(Object.values(stateRef.current.tabs));
    applyRemoteProjection(others);
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
        for (const tab of managersToAttach(payload, peer.tabs)) attachRemoteTab(workspaceId, peer, tab);
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

  return { attachRemoteTab, detachRemoteTab };
}
