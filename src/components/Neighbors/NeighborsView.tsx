import { useEffect, useRef, useState, type CSSProperties, type KeyboardEvent } from "react";
import { Plus, RefreshCw, UserRound } from "lucide-react";
import { useAccount } from "../Settings/useAccount";
import { accountTrayModel } from "../Titlebar/accountTrayModel";
import { AccountAvatar } from "../Titlebar/AccountAvatar";
import { accountInitials } from "../Settings/accountModel";
import { BusySpinner, BusyState } from "../shared/BusyState";
import { Button } from "../shared/Button";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { UI_PANEL, UI_TEXT } from "../shared/design";
import { useMuxSelector } from "../../store/multiplexer/selectorStore";
import { AddNeighborDialog } from "./AddNeighborDialog";
import { NeighborPanel, NeighborRequestPanel } from "./NeighborPanel";
import { askGeneralAgent, focusTab, permissionInterviewTask, talkAboutTask, workspaceOfTab, type NeighborSubject } from "./neighborAgent";
import type { NeighborClient } from "./neighborClient";
import { neighborhoodNodes, STATUS_LABEL, type NeighborhoodNode } from "./neighborhoodModel";
import { neighborName, type Neighbor } from "./neighborModel";
import { neighborRuntimeClient, type NeighborRuntimeClient } from "./neighborRuntimeClient";
import { referralStatus } from "./referralTrial";
import { useNeighborChannel } from "./useNeighborChannel";
import { useNeighbors } from "./useNeighbors";

const openPlans = () => window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel: "settings:plans" } }));

function useTabMap() {
  const workspaces = useMuxSelector(state => state.workspaces);
  const tabs = useMuxSelector(state => state.tabs);
  return { workspaces, tabs };
}

function SelectedNeighbor({ owner, neighbor, locked, pending, runtimeClient, onTalkAbout, onInterview, onLimit, onRemove, onClose }: {
  owner: string; neighbor: Neighbor; locked: boolean; pending: boolean; runtimeClient: NeighborRuntimeClient;
  onTalkAbout: () => void; onInterview: () => void;
  onLimit: (limit: number) => Promise<boolean>; onRemove: () => Promise<boolean>; onClose: () => void;
}) {
  const channel = useNeighborChannel(owner, neighbor.friendship_id, runtimeClient);
  const tabMap = useTabMap();
  const gatewayId = channel.status?.bindings[neighbor.friendship_id] ?? null;
  const gatewayTab = gatewayId ? tabMap.tabs[gatewayId] : undefined;
  return <NeighborPanel neighbor={neighbor} name={neighborName(neighbor)} locked={locked} pending={pending} channel={channel}
    gateway={gatewayId && gatewayTab ? { title: gatewayTab.title } : null}
    onOpenGateway={() => { if (gatewayId) focusTab(gatewayId, workspaceOfTab(tabMap, gatewayId)); }}
    onTalkAbout={onTalkAbout} onInterview={onInterview} onLimit={onLimit} onRemove={onRemove} onPlans={openPlans} onClose={onClose} />;
}

function moveFocus(event: KeyboardEvent<HTMLDivElement>) {
  const step = event.key === "ArrowRight" || event.key === "ArrowDown" ? 1 : event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 0;
  if (!step) return;
  const nodes = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>("[data-neighbor-node]:not(:disabled)"));
  const index = nodes.indexOf(document.activeElement as HTMLButtonElement);
  if (index < 0 || nodes.length === 0) return;
  event.preventDefault();
  nodes[(index + step + nodes.length) % nodes.length].focus();
}

export function NeighborsView({ client, runtimeClient = neighborRuntimeClient }: { client?: NeighborClient; runtimeClient?: NeighborRuntimeClient }) {
  const account = useAccount();
  const identity = account.model.kind === "signed_in" ? account.model.identity.uid : null;
  return <NeighborhoodPage key={identity} identity={identity} account={account} client={client} runtimeClient={runtimeClient} />;
}

function NeighborhoodPage({ identity, account, client, runtimeClient }: {
  identity: string | null; account: ReturnType<typeof useAccount>; client?: NeighborClient; runtimeClient: NeighborRuntimeClient;
}) {
  const neighbors = useNeighbors(identity, client);
  const tabMap = useTabMap();
  const me = accountTrayModel(account.model, null);
  const [selected, setSelected] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [agentError, setAgentError] = useState<string | null>(null);
  const awaitingAccept = useRef<string | null>(null);
  const list = neighbors.list;
  const paid = list?.access === "paid";
  const nodes = identity ? neighborhoodNodes(list, list !== null) : [];
  const friend = list?.friends.find(entry => entry.friendship_id === selected);
  const request = list?.incoming.find(entry => entry.request_id === selected);
  const referral = neighbors.referral && list ? referralStatus(neighbors.referral, list.access, seconds => new Date(seconds * 1000).toLocaleString()) : null;

  const ask = (text: string) => {
    setAgentError(null);
    void askGeneralAgent(tabMap, text).catch((failure: unknown) => setAgentError(failure instanceof Error ? failure.message : "The General Agent could not be reached."));
  };
  const interview = (subject: NeighborSubject) => ask(permissionInterviewTask(subject));

  // An accepted request becomes a connection on the next list; that is when its id exists for the interview.
  useEffect(() => {
    const uid = awaitingAccept.current;
    const accepted = uid ? list?.friends.find(entry => entry.uid === uid) : undefined;
    if (!accepted) return;
    awaitingAccept.current = null;
    setSelected(accepted.friendship_id);
    interview({ name: neighborName(accepted), friendshipId: accepted.friendship_id });
  }, [list]);

  const people = (list?.friends.length ?? 0);
  const requests = (list?.incoming.length ?? 0);
  const panelOpen = Boolean(friend || request);

  return <div className="neighbors-view absolute inset-0 flex flex-col bg-[var(--ct-bg)] pt-[var(--ct-safe-top,0px)]">
    <div className={`${UI_PANEL.toolbar} ct-section-toolbar gap-3 px-4 py-0 text-[var(--ct-fg)] whitespace-nowrap`}>
      <strong>Neighbors</strong>
      <span className={`${UI_TEXT.meta} opacity-70`}>
        {identity && list ? `${people} ${people === 1 ? "neighbor" : "neighbors"}${requests ? `, ${requests} ${requests === 1 ? "request" : "requests"}` : ""}` : null}
      </span>
      {identity && <button type="button" className="neighbors-refresh ct-liquid-interactive" aria-label="Refresh Neighbors" title="Refresh Neighbors"
        disabled={neighbors.pending} onClick={neighbors.refresh}>
        <BusySpinner icon={RefreshCw} size={13} spinning={neighbors.pending} />
      </button>}
    </div>
    <div className="neighbors-body" data-panel={panelOpen || undefined}>
      <div className="neighbors-canvas">
        {(neighbors.error || agentError) && <div className="neighbors-notices">
          {neighbors.error && <InlineErrorNotice error={neighbors.error} operation="managing your Neighbors" role="alert" />}
          {agentError && <InlineErrorNotice error={agentError} operation="asking your General Agent" role="alert" />}
        </div>}
        <div className="neighbors-stage" role="group" aria-label="Your neighborhood" onKeyDown={moveFocus}>
          <svg className="neighbors-edges" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
            {nodes.map(node => <line key={node.id} className="neighbor-edge" data-kind={node.kind} data-active={node.id === selected || undefined}
              x1={50} y1={50} x2={node.x} y2={node.y} vectorEffect="non-scaling-stroke" />)}
          </svg>
          <div className="neighbor-me" data-signed-in={identity ? true : undefined}>
            <span className="neighbor-me__ring">
              {me.kind === "signed_in" ? <AccountAvatar url={me.avatarUrl} initial={me.initial} label="" size="node" />
                : <span className="neighbor-me__ghost"><UserRound size={26} aria-hidden="true" /></span>}
            </span>
            {me.kind === "signed_in" ? <span className="neighbor-node__name">You</span>
              : <div className="neighbor-me__signin">
                <p className="neighbor-me__title">Sign in to meet your neighbors</p>
                <p className="neighbor-note">Your agents can message a friend's agents, end-to-end encrypted. Invited? Sign in with that email to start your 7-day trial, no card needed.</p>
                <Button size="sm" variant="primary" disabled={account.model.kind === "pending"} loading={account.model.kind === "pending"}
                  onClick={() => { void account.signIn(); }}>Sign in or create account</Button>
              </div>}
          </div>
          {nodes.map((node, index) => <NodeButton key={node.id} node={node} index={index} selected={node.id === selected}
            canInvite={list?.can_invite} onSelect={() => {
              if (node.kind === "add") {
                if (list?.can_invite) setAdding(true);
                else if (list?.can_invite === false) openPlans();
                return;
              }
              setSelected(current => current === node.id ? null : node.id);
            }} />)}
          {identity && list === null && !neighbors.error && <div className="neighbors-loading"><BusyState inline label="Loading Neighbors" phase="work" /></div>}
        </div>
        {identity && list && people + requests === 0 && <p className="neighbors-caption">Add someone you trust. Your agents can then ask theirs for help, and you decide what they may share.</p>}
        {referral && <div role="status" className="ct-card ct-card--static neighbors-referral">
          <p className="ct-card__meta">{referral}</p>
          {neighbors.referral?.trial_end !== null && neighbors.referral?.trial_end !== undefined && <Button size="sm" variant="accent" onClick={openPlans}>Subscribe</Button>}
        </div>}
      </div>
      {friend && identity && <SelectedNeighbor key={friend.friendship_id} owner={identity} neighbor={friend} locked={!paid} pending={neighbors.pending}
        runtimeClient={runtimeClient}
        onTalkAbout={() => ask(talkAboutTask({ name: neighborName(friend), friendshipId: friend.friendship_id }))}
        onInterview={() => interview({ name: neighborName(friend), friendshipId: friend.friendship_id })}
        onLimit={per_hour => neighbors.apply({ operation: "set_limit", friendship_id: friend.friendship_id, per_hour })}
        onRemove={() => neighbors.apply({ operation: "remove", friendship_id: friend.friendship_id })}
        onClose={() => setSelected(null)} />}
      {request && <NeighborRequestPanel request={{ ...request, name: neighborName(request) }} pending={neighbors.pending}
        onAccept={() => {
          awaitingAccept.current = request.uid;
          void neighbors.apply({ operation: "respond", request_id: request.request_id, accept: true }).then(ok => {
            if (ok) setSelected(null);
            else awaitingAccept.current = null;
          });
        }}
        onDecline={() => { void neighbors.apply({ operation: "respond", request_id: request.request_id, accept: false }).then(ok => { if (ok) setSelected(null); }); }}
        onClose={() => setSelected(null)} />}
    </div>
    {adding && list?.can_invite && <AddNeighborDialog pending={neighbors.pending} error={neighbors.error}
      onClose={() => setAdding(false)}
      onSend={async email => {
        const ok = await neighbors.apply({ operation: "request", email });
        if (ok) { setAdding(false); interview({ name: email, email }); }
        return ok;
      }} />}
  </div>;
}

function NodeButton({ node, index, selected, canInvite, onSelect }: {
  node: NeighborhoodNode; index: number; selected: boolean; canInvite: boolean | undefined; onSelect: () => void;
}) {
  const style = { left: `${node.x}%`, top: `${node.y}%`, "--neighbor-from-x": `${(50 - node.x) * 2}%`, "--neighbor-from-y": `${(50 - node.y) * 2}%`,
    "--neighbor-delay": `${index * 45}ms` } as CSSProperties;
  if (node.kind === "add") {
    const label = canInvite === false ? "Upgrade to add Neighbors" : "Add Neighbor";
    return <button type="button" data-neighbor-node className="neighbor-node neighbor-node--add" style={style}
      disabled={canInvite === undefined} title={canInvite === undefined ? "Invitation eligibility is unavailable. Refresh Neighbors to check again." : label}
      onClick={onSelect}>
      <span className="neighbor-node__disc" aria-hidden="true"><Plus size={20} /></span>
      <span className="neighbor-node__name">{label}</span>
    </button>;
  }
  return <button type="button" data-neighbor-node className="neighbor-node" style={style} data-status={node.status}
    aria-pressed={selected} aria-label={`${node.name}, ${STATUS_LABEL[node.status]}`} onClick={onSelect}>
    <span className="neighbor-node__disc"><AccountAvatar url={node.photoURL} initial={accountInitials(node.name)} label="" size="node" /></span>
    <span className="neighbor-node__name">{node.name}</span>
    <span className="neighbor-status"><span className="neighbor-dot" data-tone={node.status === "connected" ? "ok" : node.status === "request" ? "progress" : "muted"} aria-hidden="true" />{STATUS_LABEL[node.status]}</span>
  </button>;
}
