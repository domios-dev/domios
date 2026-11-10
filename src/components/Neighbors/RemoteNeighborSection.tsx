import { useEffect, useState } from "react";
import { useMuxSelector } from "../../store/multiplexer/hooks";
import { SidebarDisclosure } from "../Sidebar/SidebarDisclosure";
import { SurfaceOwnerBlock } from "../Sidebar/SurfaceAttachments";
import { publishNeighborGateways } from "./neighborGateways";
import { focusTab, workspaceOfTab } from "./neighborAgent";
import { useNeighborProjection } from "./neighborProjection";
import { ConciergeRow, conciergeModelLine } from "./SidebarNeighborsSection";
import { tabActivity } from "../../lib/tabActivity";
import { FriendRow } from "./FriendRow";
import { openNeighborPage } from "./neighborFocus";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";

export function RemoteNeighborSection({ peer, uiFontScale, touchMode, activeChat, activeTabId, onNavigateToTab }: {
  peer: string; uiFontScale?: number; touchMode: boolean; activeChat?: string | null; activeTabId?: string | null; onNavigateToTab?: () => void;
}) {
  const tabs = useMuxSelector(state => state.tabs);
  const workspaces = useMuxSelector(state => state.workspaces);
  const statuses = useMuxSelector(state => state.tabStatuses);
  const projection = useNeighborProjection(peer, Object.keys(tabs).sort().join(","));
  const [expanded, setExpanded] = useState(false);
  const bindingKey = JSON.stringify(Object.fromEntries(Object.entries(projection).map(([id, value]) => [id, value.tab_id])));
  useEffect(() => {
    publishNeighborGateways(JSON.parse(bindingKey) as Record<string, string>);
    return () => publishNeighborGateways({});
  }, [bindingKey]);
  const friends = Object.entries(projection).filter(([, value]) => tabs[value.tab_id]);
  const text = touchMode ? pickUiFontScaleValue(uiFontScale, ["text-[13px]", "text-[14px]", "text-[15px]", "text-base"])
    : pickUiFontScaleValue(uiFontScale, ["text-[11px]", "text-xs", "text-sm", "text-base"]);
  if (!friends.length) return null;
  return <section aria-label="Neighbors" className="sidebar-neighbors-section">
    <SidebarDisclosure expanded={expanded} touchMode={touchMode} onClick={() => setExpanded(value => !value)} className="sidebar-structural-header w-full">
      <span className="sidebar-structural-label truncate">Neighbors</span>
    </SidebarDisclosure>
    {expanded && <ul aria-label="Your Neighbors">{friends.map(([id, value]) => {
      const tab = tabs[value.tab_id];
      const paused = value.paused === true;
      const line = paused ? "disabled" : value.access === "paid" ? "connected" : value.access === "locked" ? "paused by plan" : "concierge assigned";
      return <li key={id}><SurfaceOwnerBlock ownerId={`neighbor:${id}`} attachments={[tab]} compact={false}
        renderOwner={() => <FriendRow friendshipId={id} name={value.friend_display_name} photoURL={value.friend_photo_url ?? null}
          uiFontScale={uiFontScale} touchMode={touchMode} active={activeChat === id} paused={paused} line={line}
          tone={!paused && value.access === "paid" ? "ok" : "muted"}
          items={[{ label: `${value.friend_display_name}'s settings`, onClick: () => openNeighborPage(id) }]} />}
        renderAttachment={() => <ConciergeRow tab={tab} friendName={value.friend_display_name} text={text} touchMode={touchMode} paused={paused}
          active={tab.id === activeTabId} subline={paused ? "disabled" : conciergeModelLine(tab)} working={!paused && tabActivity(tab, statuses[tab.id]).active}
          onOpen={() => { focusTab(tab.id, workspaceOfTab({ workspaces, tabs }, tab.id)); onNavigateToTab?.(); }} />} /></li>;
    })}</ul>}
  </section>;
}
