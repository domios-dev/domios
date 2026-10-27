import { useEffect, useState } from "react";
import { useAccount } from "../Settings/useAccount";
import { AccountAvatar } from "../Titlebar/AccountAvatar";
import { accountInitials } from "../Settings/accountModel";
import { SidebarStructuralSection } from "../Sidebar/SidebarStructuralSection";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";
import { useMuxSelector } from "../../store/multiplexer";
import { focusTab, workspaceOfTab } from "./neighborAgent";
import type { NeighborClient } from "./neighborClient";
import { openNeighbor } from "./neighborFocus";
import { neighborName } from "./neighborModel";
import { neighborRuntimeClient, type NeighborRuntimeClient } from "./neighborRuntimeClient";
import { useNeighbors } from "./useNeighbors";

const STORAGE_KEY = "codeterm.sidebar.neighbors-expanded";

function readExpanded(): boolean {
  try { return localStorage.getItem(STORAGE_KEY) !== "false"; }
  catch { return true; }
}

/** Your connected Neighbors as sidebar rows; the section stays away until you have one. */
export function SidebarNeighborsSection({ uiFontScale, touchMode, onNavigateToTab, enabled = true, client, runtimeClient = neighborRuntimeClient }: {
  uiFontScale?: number; touchMode: boolean; onNavigateToTab?: () => void; enabled?: boolean; client?: NeighborClient; runtimeClient?: NeighborRuntimeClient;
}) {
  const { model } = useAccount();
  const identity = model.kind === "signed_in" ? model.identity.uid : null;
  const neighbors = useNeighbors(identity, client);
  const workspaces = useMuxSelector(state => state.workspaces);
  const tabs = useMuxSelector(state => state.tabs);
  const [expanded, setExpanded] = useState(readExpanded);
  const [bindings, setBindings] = useState<Record<string, string>>({});
  const friends = neighbors.list?.friends ?? [];
  const paid = neighbors.list?.access === "paid";
  const friendKey = friends.map(friend => friend.friendship_id).join(",");

  useEffect(() => {
    if (!friendKey) return;
    const controller = new AbortController();
    runtimeClient.status(controller.signal).then(status => setBindings(status.bindings), () => setBindings({}));
    return () => controller.abort();
  }, [friendKey, runtimeClient]);

  if (friends.length === 0) return null;
  const text = touchMode ? pickUiFontScaleValue(uiFontScale, ["text-[13px]", "text-[14px]", "text-[15px]", "text-base"])
    : pickUiFontScaleValue(uiFontScale, ["text-[11px]", "text-xs", "text-sm", "text-base"]);
  return <SidebarStructuralSection label="NEIGHBORS" ariaLabel="Neighbors" testId="neighbors-section-toggle" expanded={expanded}
    onToggle={() => setExpanded(value => {
      try { localStorage.setItem(STORAGE_KEY, String(!value)); } catch { /* Storage can be unavailable in a private viewer. */ }
      return !value;
    })}>
    {expanded && !enabled && <p role="status" className="sidebar-neighbors-off">Neighbors is off</p>}
    {expanded && <ul aria-label="Your Neighbors" className="sidebar-neighbors" data-off={enabled ? undefined : true}>{friends.map(friend => {
      const name = neighborName(friend);
      const gateway = bindings[friend.friendship_id];
      // While Neighbors is off nothing reaches their tab, so a row only opens the neighborhood.
      const gatewayTab = enabled && gateway ? tabs[gateway] : undefined;
      const status = !enabled ? "off" : paid ? "connected" : "paused by plan";
      return <li key={friend.friendship_id}
        className={`sidebar-tab sidebar-tab-dimmed group flex items-center ${touchMode ? "min-h-11 pl-5 pr-2 py-2" : "pl-4 pr-4 py-1.5"} hover:bg-[var(--ct-hover)]`}>
        <button type="button" className={`sidebar-tab-activation flex flex-1 items-center min-w-0 border-0 bg-transparent p-0 text-left text-inherit cursor-pointer ${touchMode ? "gap-2.5" : "gap-2"}`}
          aria-label={gatewayTab ? `Open ${name}'s conversation in ${gatewayTab.title}` : `Open ${name} in Neighbors`}
          onClick={() => {
            if (!gatewayTab) { openNeighbor(friend.friendship_id); return; }
            focusTab(gateway, workspaceOfTab({ workspaces, tabs }, gateway));
            onNavigateToTab?.();
          }}>
          <span className={`sidebar-tab-body flex-1 min-w-0 ${touchMode ? "gap-x-2.5" : "gap-x-2"}`}>
            <span className="sidebar-tab-body__icon sidebar-neighbor-avatar"><AccountAvatar url={friend.photoURL} initial={accountInitials(name)} label="" size="node" /></span>
            <span className="sidebar-tab-body__title flex min-w-0 items-center gap-1.5">
              <span className={`${text} block truncate min-w-0 text-[var(--ct-fg)]`}>{name}</span>
            </span>
            <span className="sidebar-tab-body__sub neighbor-status text-[10px] leading-3">
              <span className="neighbor-dot" data-tone={enabled && paid ? "ok" : "muted"} aria-hidden="true" />{gatewayTab ? `${status} · ${gatewayTab.title}` : status}
            </span>
          </span>
        </button>
      </li>;
    })}</ul>}
  </SidebarStructuralSection>;
}
