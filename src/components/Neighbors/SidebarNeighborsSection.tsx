import { createRef, useEffect, useMemo, useRef, useState, type RefObject } from "react";
import { Bot, Pause } from "lucide-react";
import { AgentIcon } from "../AgentIcon";
import { agentModelForBadge } from "../../lib/agentLabel";
import { shortModelName } from "../../lib/agentModelName";
import { ActiveTabPill } from "../Sidebar/ActiveTabPill";
import type { Tab } from "../../lib/types";
import { useAccount } from "../Settings/useAccount";
import { AccountAvatar } from "../Titlebar/AccountAvatar";
import { accountInitials } from "../Settings/accountModel";
import { SidebarDisclosure } from "../Sidebar/SidebarDisclosure";
import { SurfaceOwnerBlock } from "../Sidebar/SurfaceAttachments";
import { DropdownMenu, type DropdownMenuItem } from "../shared/DropdownMenu";
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
import { missingConciergeLine, useConciergeStatus } from "./useConciergeStatus";
import { resetNeighborStatus, useNeighborStatus } from "./neighborStatusStore";
import { resetPausedFriends, setFriendPaused, usePausedFriends } from "./neighborPause";
import { FriendReceiveConfirm, friendReceiveItems, friendReceivingLine, useFriendReceiveRequest } from "./neighborFriendReceiving";
import { friendReceiverState } from "./useReceiverState";

const ROW = "sidebar-tab sidebar-tab-dimmed group flex items-center hover:bg-[var(--ct-hover)]";

/** Your connected Neighbors as sidebar rows, each with its concierge tucked under it; hidden until you have one. */
export function SidebarNeighborsSection({ uiFontScale, touchMode, onNavigateToTab, enabled = true, activeChat = null, activeTabId = null, client, runtimeClient = neighborRuntimeClient,
  conciergeClient = neighborConciergeClient }: {
  uiFontScale?: number; touchMode: boolean; onNavigateToTab?: () => void; enabled?: boolean; activeChat?: string | null;
  /** The session tab shown in the workspace view, the same source ordinary tab rows highlight from. */
  activeTabId?: string | null; client?: NeighborClient;
  runtimeClient?: NeighborRuntimeClient; conciergeClient?: NeighborConciergeClient;
}) {
  const { model } = useAccount();
  const identity = model.kind === "signed_in" ? model.identity.uid : null;
  const neighbors = useNeighbors(identity, client);
  const workspaces = useMuxSelector(state => state.workspaces);
  const tabs = useMuxSelector(state => state.tabs);
  const tabStatuses = useMuxSelector(state => state.tabStatuses);
  const [expanded, setExpanded] = useState(false);
  const [bindings, setBindings] = useState<Record<string, string>>({});
  const isPaused = usePausedFriends(conciergeClient);
  const receive = useFriendReceiveRequest();
  const lastIdentity = useRef<string | null>(null);
  // Each friend row is its menu's long-press and right-click target, as an ordinary tab row is.
  const rowRefs = useRef(new Map<string, RefObject<HTMLDivElement | null>>());
  const rowRef = (friendship: string) => {
    let ref = rowRefs.current.get(friendship);
    if (!ref) { ref = createRef<HTMLDivElement>(); rowRefs.current.set(friendship, ref); }
    return ref;
  };
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
  const concierges = useConciergeStatus(runtimeClient, friends.length > 0 && enabled);

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
    detail => publishNotice({ kind: "toast", severity: "Error", id: `neighbor-pause:${friend.friendship_id}`, title: paused ? "Could not enable" : "Could not disable", detail }));
  const text = touchMode ? pickUiFontScaleValue(uiFontScale, ["text-[13px]", "text-[14px]", "text-[15px]", "text-base"])
    : pickUiFontScaleValue(uiFontScale, ["text-[11px]", "text-xs", "text-sm", "text-base"]);
  const rowPadding = touchMode ? "min-h-11 pl-5 pr-2 py-2" : "pl-4 pr-4 py-1.5";
  const editWithGeneral = (friend: CachedFriend) => {
    void askGeneralAgent({ workspaces, tabs }, permissionInterviewTask({ name: neighborName(friend), friendshipId: friend.friendship_id }))
      .catch((failure: unknown) => publishNotice({ kind: "toast", severity: "Error", id: `neighbor-general:${friend.friendship_id}`,
        title: "General Agent unavailable", detail: failure instanceof Error ? failure.message : "Open General and try again." }));
  };

  return <section aria-label="Neighbors" className="sidebar-neighbors-section">
    <SidebarDisclosure expanded={expanded} touchMode={touchMode} onClick={() => setExpanded(value => !value)}
      className="sidebar-structural-header w-full">
      <span className="sidebar-structural-label truncate">Neighbors</span>
      {loading && <span className="sidebar-neighbors-loading" role="status" aria-label="Refreshing Neighbors"><ActivityDots size="small" tone="muted" active /></span>}
    </SidebarDisclosure>
    <div className="sidebar-neighbors-section__items mt-0.5 overflow-y-auto">
    {expanded && !enabled && <p role="status" className="sidebar-neighbors-off">Neighbors is off</p>}
    {expanded && <ul aria-label="Your Neighbors" className="sidebar-neighbors" data-off={enabled ? undefined : true}>{friends.map(friend => {
      const name = neighborName(friend);
      const conciergeId = allBindings[friend.friendship_id];
      const concierge = conciergeId ? tabs[conciergeId] : undefined;
      // A bound concierge wins over a stale missing state.
      const missing = enabled && !concierge && concierges[friend.friendship_id]?.concierge_missing ? concierges[friend.friendship_id] : null;
      const paused = isPaused(friend.friendship_id, listedPaused(friend));
      // A friend received on another machine says where; its concierge there, not here, is the one that answers.
      const receiver = enabled ? friendReceiverState(status, friend.friendship_id) : null;
      const elsewhere = receiver !== null && !receiver.receiving_here;
      const line = !enabled ? "off" : paused ? "disabled" : elsewhere ? friendReceivingLine(receiver)
        : missing ? missingConciergeLine(missing).replace("No concierge on this machine", "no concierge here") : paid ? "connected" : "paused by plan";
      const pauseItem = { label: `${paused ? "Enable" : "Disable"} ${name}`, onClick: () => togglePaused(friend, paused) };
      const receiveItems = friendReceiveItems(receiver, operation => receive.open(friend.friendship_id, operation));
      return <li key={friend.friendship_id}>
        <SurfaceOwnerBlock ownerId={`neighbor:${friend.friendship_id}`} attachments={concierge ? [concierge] : []} compact={false}
          renderOwner={() => <div ref={rowRef(friend.friendship_id)} className={`${ROW} ${rowPadding}`} data-active={activeChat === friend.friendship_id || undefined} data-paused={paused || undefined}>
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
                  <span className="neighbor-dot" data-tone={paused || elsewhere ? "muted" : missing ? "progress" : enabled && paid ? "ok" : "muted"} aria-hidden="true" />{line}
                </span>
              </span>
            </button>
            <DropdownMenu className={touchMode ? "sr-only" : "sidebar-tab-kebab"} contextMenuTargetRef={rowRef(friend.friendship_id)} menuLabel={`${name} actions`}
              items={[pauseItem, ...receiveItems, { label: `${name}'s settings`, onClick: () => openNeighborPage(friend.friendship_id) }]}
              trigger={<RowMenuButton label={`Actions for ${name}`} title="Friend actions" touchMode={touchMode} />} />
          </div>}
          renderAttachment={tab => <ConciergeRow tab={tab} friendName={name} text={text} touchMode={touchMode} paused={paused} pauseItem={pauseItem} active={tab.id === activeTabId}
            subline={paused ? "disabled" : conciergeModelLine(tab)} working={!paused && tabActivity(tab, tabStatuses[tab.id]).active}
            onOpen={() => { focusTab(tab.id, workspaceOfTab({ workspaces, tabs }, tab.id)); onNavigateToTab?.(); }}
            onChangeAgent={() => openNeighborPage(friend.friendship_id, "concierge")}
            onViewPermissions={() => openNeighborPage(friend.friendship_id, "permissions")}
            onEditWithGeneral={() => editWithGeneral(friend)} receiveItems={receiveItems} />} />
        {receive.request?.friendship === friend.friendship_id && receiver && <div className="sidebar-neighbor-receive">
          <FriendReceiveConfirm request={receive.request} receiver={receiver} friendName={name} targetMachine={status?.machine_label}
            client={runtimeClient} onClose={receive.close} /></div>}
      </li>;
    })}</ul>}
  </div>
  </section>;
}

/** The concierge's model (and effort) is its stable subline; what it is doing shows only as the working dots. */
export function conciergeModelLine(tab: Tab): string {
  const model = agentModelForBadge({
    sessionModel: tab.agent_session_model,
    providerResolvedModel: tab.agent_provider_resolved_model,
    requestedModel: tab.agent_requested_model,
  });
  const effort = tab.reasoning_effort_identity?.actual ?? tab.reasoning_effort_identity?.resolved ?? tab.reasoning_effort_identity?.requested;
  const name = model ? shortModelName(model) ?? model : tab.agent_type ?? "Concierge";
  return effort ? `${name} · ${effort}` : name;
}

function ConciergeRow({ tab, friendName, text, touchMode, subline, working, paused, pauseItem, receiveItems, active, onOpen, onChangeAgent, onViewPermissions, onEditWithGeneral }: {
  tab: Tab; friendName: string; text: string; touchMode: boolean; subline: string; working: boolean; paused: boolean; pauseItem: { label: string; onClick: () => void };
  receiveItems: DropdownMenuItem[]; active: boolean; onOpen: () => void; onChangeAgent: () => void; onViewPermissions: () => void; onEditWithGeneral: () => void;
}) {
  const label = `${friendName}'s concierge`;
  const row = useRef<HTMLDivElement>(null);
  // An open concierge chat highlights like any open tab: the same pill, full-strength text, aria-current.
  return <div ref={row} className={`${active ? ROW.replace(" sidebar-tab-dimmed", "") : ROW} relative sidebar-neighbor-concierge ${touchMode ? "min-h-9 pl-5 pr-2 py-1" : "pr-4 py-0.5"}`}
    data-paused={paused || undefined} data-tab-active={active || undefined}>
    {active && <ActiveTabPill />}
    <button type="button" className="sidebar-tab-activation relative z-[1] flex flex-1 items-center min-w-0 gap-2 border-0 bg-transparent p-0 text-left text-inherit cursor-pointer"
      aria-label={`Open ${label}`} aria-current={active ? "page" : undefined} title={tab.title} onClick={onOpen}>
      <span className="sidebar-tab-body flex-1 min-w-0 gap-x-2">
        <span data-testid="tab-tab-icon" className="sidebar-tab-body__icon text-[var(--ct-muted-fg)]">{tab.agent_type
          ? <AgentIcon providerId={tab.agent_type} className="text-[12px]" title={tab.agent_type} />
          : <Bot size={12} aria-hidden="true" />}</span>
        <span className="sidebar-tab-body__title flex min-w-0 items-center gap-1.5">
          <span className={`${text} block truncate min-w-0 ${active ? "font-medium text-[var(--ct-fg)]" : "text-[var(--ct-muted-fg)]"}`}>{label}</span>
          {working && <span data-testid="concierge-working" role="img" aria-label={`${label} working`} className="inline-flex items-center shrink-0">
            <ActivityDots size="small" tone="muted" active /></span>}
        </span>
        <span className="sidebar-tab-body__sub block truncate text-[10px] leading-3 text-[var(--ct-muted-fg)]">{subline}</span>
      </span>
    </button>
    <DropdownMenu className={touchMode ? "sr-only" : "sidebar-tab-kebab"} contextMenuTargetRef={row} menuLabel={`${label} actions`}
      items={[
        { label: "Change agent", onClick: onChangeAgent },
        { label: "View permissions", onClick: onViewPermissions },
        { label: "Edit with General", onClick: onEditWithGeneral },
        ...receiveItems,
        pauseItem,
      ]}
      trigger={<RowMenuButton label={`Actions for ${label}`} title="Concierge actions" touchMode={touchMode} />} />
  </div>;
}
