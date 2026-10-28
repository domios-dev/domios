import { useEffect, useMemo, useRef, useState } from "react";
import { Bot, Pause } from "lucide-react";
import type { Tab } from "../../lib/types";
import { useAccount } from "../Settings/useAccount";
import { AccountAvatar } from "../Titlebar/AccountAvatar";
import { accountInitials } from "../Settings/accountModel";
import { SidebarStructuralSection } from "../Sidebar/SidebarStructuralSection";
import { SurfaceOwnerBlock } from "../Sidebar/SurfaceAttachments";
import { DropdownMenu } from "../shared/DropdownMenu";
import { RowMenuButton } from "../shared/RowMenuButton";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";
import { tabActivity } from "../../lib/tabActivity";
import { publishNotice } from "../../lib/notices";
import { useMuxSelector } from "../../store/multiplexer";
import { askGeneralAgent, focusTab, permissionInterviewTask, workspaceOfTab } from "./neighborAgent";
import type { NeighborClient } from "./neighborClient";
import { neighborConciergeClient, type NeighborConciergeClient } from "./neighborConciergeClient";
import { openNeighborPage, openNeighbor } from "./neighborFocus";
import { publishNeighborGateways } from "./neighborGateways";
import { listedPaused, neighborName } from "./neighborModel";
import { neighborRuntimeClient, type NeighborRuntimeClient } from "./neighborRuntimeClient";
import { useNeighbors } from "./useNeighbors";
import { clearNeighborCaches, readNeighborCache, writeNeighborCache, type CachedFriend, type CachedNeighbors } from "./neighborListCache";
import { ActivityDots } from "../shared/ActivityDots";
import { receivingLine, useReceiverState } from "./useReceiverState";
import { missingConciergeLine, useConciergeStatus } from "./useConciergeStatus";
import { resetNeighborStatus, useNeighborStatus } from "./neighborStatusStore";
import { resetPausedFriends, setFriendPaused, usePausedFriends } from "./neighborPause";

const STORAGE_KEY = "codeterm.sidebar.neighbors-expanded";
const ROW = "sidebar-tab sidebar-tab-dimmed group flex items-center hover:bg-[var(--ct-hover)]";

function readExpanded(): boolean {
  try { return localStorage.getItem(STORAGE_KEY) !== "false"; }
  catch { return true; }
}

/** Your connected Neighbors as sidebar rows, each with its concierge tucked under it; hidden until you have one. */
export function SidebarNeighborsSection({ uiFontScale, touchMode, onNavigateToTab, enabled = true, activeChat = null, client, runtimeClient = neighborRuntimeClient,
  conciergeClient = neighborConciergeClient }: {
  uiFontScale?: number; touchMode: boolean; onNavigateToTab?: () => void; enabled?: boolean; activeChat?: string | null; client?: NeighborClient;
  runtimeClient?: NeighborRuntimeClient; conciergeClient?: NeighborConciergeClient;
}) {
  const { model } = useAccount();
  const identity = model.kind === "signed_in" ? model.identity.uid : null;
  const neighbors = useNeighbors(identity, client);
  const workspaces = useMuxSelector(state => state.workspaces);
  const tabs = useMuxSelector(state => state.tabs);
  const tabStatuses = useMuxSelector(state => state.tabStatuses);
  const [expanded, setExpanded] = useState(readExpanded);
  const [bindings, setBindings] = useState<Record<string, string>>({});
  const isPaused = usePausedFriends(conciergeClient);
  const lastIdentity = useRef<string | null>(null);
  // Until the fresh list arrives, draw the friends this account had last time; an empty past keeps the section away.
  const cached = useMemo(() => readNeighborCache(identity), [identity]);
  const loading = identity !== null && neighbors.list === null;
  const shown: CachedNeighbors | null = neighbors.list ?? (loading ? cached : null);
  const friends: CachedFriend[] = shown?.friends ?? [];
  const paid = shown?.access === "paid";
  useEffect(() => {
    if (model.kind === "signed_out") clearNeighborCaches();
    else if (identity) clearNeighborCaches(identity);
    // The shared runtime status belongs to the account it was read for.
    if (model.kind === "signed_out" || (identity && lastIdentity.current && lastIdentity.current !== identity)) { resetNeighborStatus(); resetPausedFriends(); }
    if (identity) lastIdentity.current = identity;
  }, [model.kind, identity]);
  useEffect(() => { if (identity && neighbors.list) writeNeighborCache(identity, neighbors.list); }, [identity, neighbors.list]);
  const friendKey = friends.map(friend => friend.friendship_id).join(",");
  const receiver = useReceiverState(runtimeClient, friends.length > 0 && enabled);
  const concierges = useConciergeStatus(runtimeClient, friends.length > 0 && enabled);
  // Concierges only work on the receiving machine; elsewhere they wait.
  const idleNote = receiver && !receiver.receiving_here && receiver.receiving_enabled ? `idle, ${receivingLine(receiver).toLowerCase()}` : null;

  // The list row names the bound concierge; the shared runtime status and agent events fill in or update the rest.
  const status = useNeighborStatus(runtimeClient, friendKey !== "");
  const listBindings = Object.fromEntries(friends.flatMap(friend => friend.gateway_tab ? [[friend.friendship_id, friend.gateway_tab]] : []));
  const allBindings = Object.fromEntries(Object.entries({ ...listBindings, ...(status?.bindings ?? {}), ...bindings }).filter(([, tab]) => tab !== ""));
  const bindingKey = JSON.stringify(allBindings);
  useEffect(() => { publishNeighborGateways(JSON.parse(bindingKey) as Record<string, string>); }, [bindingKey]);
  useEffect(() => {
    let stop: (() => void) | undefined;
    let live = true;
    void conciergeClient.onAgentChanged((friendship, gatewayTab) => setBindings(current => {
      // An empty tab marks a removed concierge, so it also hides whatever the list or the snapshot still names.
      return { ...current, [friendship]: gatewayTab ?? "" };
    }))
      .then(unlisten => { if (live) stop = unlisten; else unlisten(); });
    return () => { live = false; stop?.(); };
  }, [conciergeClient]);

  if (friends.length === 0) return null;
  const togglePaused = (friend: CachedFriend, paused: boolean) => setFriendPaused(conciergeClient, friend.friendship_id, !paused, paused,
    detail => publishNotice({ kind: "toast", severity: "Error", id: `neighbor-pause:${friend.friendship_id}`, title: paused ? "Could not resume" : "Could not pause", detail }));
  const text = touchMode ? pickUiFontScaleValue(uiFontScale, ["text-[13px]", "text-[14px]", "text-[15px]", "text-base"])
    : pickUiFontScaleValue(uiFontScale, ["text-[11px]", "text-xs", "text-sm", "text-base"]);
  const rowPadding = touchMode ? "min-h-11 pl-5 pr-2 py-2" : "pl-4 pr-4 py-1.5";
  const editWithGeneral = (friend: CachedFriend) => {
    void askGeneralAgent({ workspaces, tabs }, permissionInterviewTask({ name: neighborName(friend), friendshipId: friend.friendship_id }))
      .catch((failure: unknown) => publishNotice({ kind: "toast", severity: "Error", id: `neighbor-general:${friend.friendship_id}`,
        title: "General Agent unavailable", detail: failure instanceof Error ? failure.message : "Open General and try again." }));
  };

  return <SidebarStructuralSection label="NEIGHBORS" ariaLabel="Neighbors" testId="neighbors-section-toggle" expanded={expanded}
    action={loading ? <span className="sidebar-neighbors-loading" role="status" aria-label="Refreshing Neighbors"><ActivityDots size="small" tone="muted" active /></span> : null}
    onToggle={() => setExpanded(value => {
      try { localStorage.setItem(STORAGE_KEY, String(!value)); } catch { /* Storage can be unavailable in a private viewer. */ }
      return !value;
    })}>
    {expanded && !enabled && <p role="status" className="sidebar-neighbors-off">Neighbors is off</p>}
    {expanded && <ul aria-label="Your Neighbors" className="sidebar-neighbors" data-off={enabled ? undefined : true}>{friends.map(friend => {
      const name = neighborName(friend);
      const conciergeId = allBindings[friend.friendship_id];
      const concierge = conciergeId ? tabs[conciergeId] : undefined;
      // A bound concierge wins over a stale missing state.
      const missing = enabled && !concierge && concierges[friend.friendship_id]?.concierge_missing ? concierges[friend.friendship_id] : null;
      const paused = isPaused(friend.friendship_id, listedPaused(friend));
      const status = !enabled ? "off" : paused ? "paused" : missing ? missingConciergeLine(missing).replace("No concierge on this machine", "no concierge here") : paid ? "connected" : "paused by plan";
      const pauseItem = { label: `${paused ? "Resume" : "Pause"} ${name}`, onClick: () => togglePaused(friend, paused) };
      return <li key={friend.friendship_id}>
        <SurfaceOwnerBlock ownerId={`neighbor:${friend.friendship_id}`} attachments={concierge ? [concierge] : []} compact={false}
          renderOwner={() => <div className={`${ROW} ${rowPadding}`} data-active={activeChat === friend.friendship_id || undefined} data-paused={paused || undefined}>
            <button type="button" className={`sidebar-tab-activation flex flex-1 items-center min-w-0 border-0 bg-transparent p-0 text-left text-inherit cursor-pointer ${touchMode ? "gap-2.5" : "gap-2"}`}
              aria-label={`Open ${name}`} aria-current={activeChat === friend.friendship_id ? "page" : undefined}
              onClick={() => openNeighbor(friend.friendship_id)}>
              <span className={`sidebar-tab-body flex-1 min-w-0 ${touchMode ? "gap-x-2.5" : "gap-x-2"}`}>
                <span data-testid="tab-tab-icon" className="sidebar-tab-body__icon sidebar-neighbor-avatar" data-paused={paused || undefined}>
                  <AccountAvatar url={friend.photoURL} initial={accountInitials(name)} label="" size="node" />
                  {paused && <span className="neighbor-pause-glyph" aria-hidden="true"><Pause size={8} /></span>}</span>
                <span className="sidebar-tab-body__title flex min-w-0 items-center gap-1.5">
                  <span className={`${text} block truncate min-w-0 text-[var(--ct-fg)]`}>{name}</span>
                </span>
                <span className="sidebar-tab-body__sub neighbor-status text-[10px] leading-3">
                  <span className="neighbor-dot" data-tone={paused ? "muted" : missing ? "progress" : enabled && paid ? "ok" : "muted"} aria-hidden="true" />{status}
                </span>
              </span>
            </button>
            <DropdownMenu className="sidebar-neighbor-concierge__menu" menuLabel={`${name} actions`}
              items={[pauseItem, { label: `${name}'s settings`, onClick: () => openNeighborPage(friend.friendship_id) }]}
              trigger={<RowMenuButton label={`Actions for ${name}`} title="Friend actions" touchMode={touchMode} />} />
          </div>}
          renderAttachment={tab => <ConciergeRow tab={tab} friendName={name} text={text} touchMode={touchMode} paused={paused} pauseItem={pauseItem}
            activity={paused ? "paused" : idleNote ?? tabActivity(tab, tabStatuses[tab.id]).label}
            onOpen={() => { focusTab(tab.id, workspaceOfTab({ workspaces, tabs }, tab.id)); onNavigateToTab?.(); }}
            onChangeAgent={() => openNeighborPage(friend.friendship_id, "concierge")}
            onViewPermissions={() => openNeighborPage(friend.friendship_id, "permissions")}
            onEditWithGeneral={() => editWithGeneral(friend)} />} />
      </li>;
    })}</ul>}
  </SidebarStructuralSection>;
}

function ConciergeRow({ tab, friendName, text, touchMode, activity, paused, pauseItem, onOpen, onChangeAgent, onViewPermissions, onEditWithGeneral }: {
  tab: Tab; friendName: string; text: string; touchMode: boolean; activity: string; paused: boolean; pauseItem: { label: string; onClick: () => void };
  onOpen: () => void; onChangeAgent: () => void; onViewPermissions: () => void; onEditWithGeneral: () => void;
}) {
  const label = `${friendName}'s concierge`;
  return <div className={`${ROW} sidebar-neighbor-concierge ${touchMode ? "min-h-9 pl-5 pr-2 py-1" : "pr-2 py-0.5"}`} data-paused={paused || undefined}>
    <button type="button" className="sidebar-tab-activation flex flex-1 items-center min-w-0 gap-2 border-0 bg-transparent p-0 text-left text-inherit cursor-pointer"
      aria-label={`Open ${label}`} title={tab.title} onClick={onOpen}>
      <span className="sidebar-tab-body flex-1 min-w-0 gap-x-2">
        <span data-testid="tab-tab-icon" className="sidebar-tab-body__icon text-[var(--ct-muted-fg)]"><Bot size={12} aria-hidden="true" /></span>
        <span className="sidebar-tab-body__title flex min-w-0 items-center gap-1.5">
          <span className={`${text} block truncate min-w-0 text-[var(--ct-muted-fg)]`}>{label}</span>
        </span>
        <span className="sidebar-tab-body__sub block truncate text-[10px] leading-3 text-[var(--ct-muted-fg)]">{activity}</span>
      </span>
    </button>
    <DropdownMenu className="sidebar-neighbor-concierge__menu" menuLabel={`${label} actions`}
      items={[
        { label: "Change agent", onClick: onChangeAgent },
        { label: "View permissions", onClick: onViewPermissions },
        { label: "Edit with General", onClick: onEditWithGeneral },
        pauseItem,
      ]}
      trigger={<RowMenuButton label={`Actions for ${label}`} title="Concierge actions" touchMode={touchMode} />} />
  </div>;
}
