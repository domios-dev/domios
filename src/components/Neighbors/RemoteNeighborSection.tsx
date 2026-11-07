import { useEffect, useState } from "react";
import { useMuxSelector } from "../../store/multiplexer/hooks";
import { SidebarDisclosure } from "../Sidebar/SidebarDisclosure";
import { SurfaceOwnerBlock } from "../Sidebar/SurfaceAttachments";
import { publishNeighborGateways } from "./neighborGateways";
import { focusTab, workspaceOfTab } from "./neighborAgent";
import { useNeighborProjection } from "./neighborProjection";
import { ConciergeRow, conciergeModelLine } from "./SidebarNeighborsSection";
import { tabActivity } from "../../lib/tabActivity";

export function RemoteNeighborSection({ peer, touchMode, activeTabId, onNavigateToTab }: {
  peer: string; touchMode: boolean; activeTabId?: string | null; onNavigateToTab?: () => void;
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
  if (!friends.length) return null;
  return <section aria-label="Neighbors" className="sidebar-neighbors-section">
    <SidebarDisclosure expanded={expanded} touchMode={touchMode} onClick={() => setExpanded(value => !value)} className="sidebar-structural-header w-full">
      <span className="sidebar-structural-label truncate">Neighbors</span>
    </SidebarDisclosure>
    {expanded && <ul aria-label="Your Neighbors">{friends.map(([id, value]) => {
      const tab = tabs[value.tab_id];
      return <li key={id}><SurfaceOwnerBlock ownerId={`neighbor:${id}`} attachments={[tab]} compact={false}
        renderOwner={() => <div className="sidebar-tab pl-4 pr-4 py-1.5">{value.friend_display_name}</div>}
        renderAttachment={() => <ConciergeRow tab={tab} friendName={value.friend_display_name} text="text-xs" touchMode={touchMode}
          active={tab.id === activeTabId} subline={conciergeModelLine(tab)} working={tabActivity(tab, statuses[tab.id]).active}
          onOpen={() => { focusTab(tab.id, workspaceOfTab({ workspaces, tabs }, tab.id)); onNavigateToTab?.(); }} />} /></li>;
    })}</ul>}
  </section>;
}
