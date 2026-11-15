import { useEffect, useState } from "react";
import { useMuxSelector } from "../../store/multiplexer/hooks";
import { SidebarDisclosure } from "../Sidebar/SidebarDisclosure";
import { SurfaceOwnerBlock } from "../Sidebar/SurfaceAttachments";
import { publishNeighborGateways } from "./neighborGateways";
import { focusTab, workspaceOfTab } from "./neighborAgent";
import { useNeighborProjection } from "./neighborProjection";
import { ConciergeRow } from "./SidebarNeighborsSection";
import { tabActivity } from "../../lib/tabActivity";
import { FriendRow } from "./FriendRow";
import { openNeighborPage } from "./neighborFocus";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";
import { friendReceivingLine } from "./neighborFriendReceiving";
import { receiverOpenItem } from "./neighborReceiverLocation";
import { conciergeNeedsAttention, conciergeStatusLine, parseConciergeStatus } from "./useConciergeStatus";

export function RemoteNeighborSection({ peer, uiFontScale, touchMode, activeChat, activeTabId, onNavigateToTab }: {
  peer: string; uiFontScale?: number; touchMode: boolean; activeChat?: string | null; activeTabId?: string | null; onNavigateToTab?: () => void;
}) {
  const tabs = useMuxSelector(state => state.tabs);
  const workspaces = useMuxSelector(state => state.workspaces);
  const statuses = useMuxSelector(state => state.tabStatuses);
  const projection = useNeighborProjection(peer, Object.keys(tabs).sort().join(","));
  const [expanded, setExpanded] = useState(false);
  const bindingKey = JSON.stringify(Object.fromEntries(Object.entries(projection).flatMap(([id, value]) => value.tab_id ? [[id, value.tab_id]] : [])));
  useEffect(() => {
    publishNeighborGateways(JSON.parse(bindingKey) as Record<string, string>);
    return () => publishNeighborGateways({});
  }, [bindingKey]);
  const friends = Object.entries(projection);
  const text = touchMode ? pickUiFontScaleValue(uiFontScale, ["text-[13px]", "text-[14px]", "text-[15px]", "text-base"])
    : pickUiFontScaleValue(uiFontScale, ["text-[11px]", "text-xs", "text-sm", "text-base"]);
  if (!friends.length) return null;
  return <section aria-label="Neighbors" className="sidebar-neighbors-section">
    <SidebarDisclosure expanded={expanded} touchMode={touchMode} onClick={() => setExpanded(value => !value)} className="sidebar-structural-header w-full">
      <span className="sidebar-structural-label truncate">Neighbors</span>
    </SidebarDisclosure>
    {expanded && <ul aria-label="Your Neighbors">{friends.map(([id, value]) => {
      const tab = value.tab_id ? tabs[value.tab_id] : undefined;
      const paused = value.paused === true;
      const receiver = value.receiver;
      const elsewhere = receiver != null && !receiver.receiving_here;
      const lifecycle = parseConciergeStatus(value.concierge);
      const needsAttention = conciergeNeedsAttention(lifecycle);
      const openThere = elsewhere ? receiverOpenItem(receiver, receiver.receiver_machine_label ?? "concierge machine") : null;
      const line = elsewhere ? friendReceivingLine(receiver) : paused ? `disabled${!receiver ? " · concierge location unknown" : ""}`
        : !receiver ? friendReceivingLine(null) : value.access === "locked" ? "paused by plan"
        : lifecycle?.state === "ready" ? "" : conciergeStatusLine(lifecycle);
      return <li key={id}><SurfaceOwnerBlock ownerId={`neighbor:${id}`} attachments={tab && !elsewhere ? [tab] : []} compact={false}
        renderOwner={() => <FriendRow friendshipId={id} name={value.friend_display_name} photoURL={value.friend_photo_url ?? null}
          uiFontScale={uiFontScale} touchMode={touchMode} active={activeChat === id} paused={paused} line={line}
          tone={!paused && receiver && !elsewhere && value.access === "paid" && lifecycle?.state === "ready" ? "ok" : needsAttention ? "progress" : "muted"}
          items={[...(openThere ? [openThere] : []), { label: `${value.friend_display_name}'s settings`, onClick: () => openNeighborPage(id) }]} />}
        renderAttachment={tab => <ConciergeRow tab={tab} friendName={value.friend_display_name} text={text} touchMode={touchMode} paused={paused}
          active={tab.id === activeTabId} subline={paused ? "disabled" : needsAttention ? conciergeStatusLine(lifecycle) : undefined} working={!paused && !needsAttention && tabActivity(tab, statuses[tab.id]).active}
          onOpen={() => { focusTab(tab.id, workspaceOfTab({ workspaces, tabs }, tab.id)); onNavigateToTab?.(); }} />} />
      </li>;
    })}</ul>}
  </section>;
}
