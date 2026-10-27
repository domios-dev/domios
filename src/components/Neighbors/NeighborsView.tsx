import { useEffect, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent } from "react";
import { Mail, Plus, RefreshCw, UserRound } from "lucide-react";
import { formatAge } from "../../lib/timeLabels";
import { useAccount } from "../Settings/useAccount";
import { accountTrayModel } from "../Titlebar/accountTrayModel";
import { AccountAvatar } from "../Titlebar/AccountAvatar";
import { accountInitials } from "../Settings/accountModel";
import { BusySpinner, BusyState } from "../shared/BusyState";
import { Button } from "../shared/Button";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { UI_PANEL, UI_TEXT } from "../shared/design";
import { useMuxSelector } from "../../store/multiplexer";
import { AddNeighborDialog } from "./AddNeighborDialog";
import { NeighborPrivacyNote, NeighborRequestPanel } from "./NeighborPanel";
import { NeighborFriendPage } from "./NeighborFriendPage";
import { askGeneralAgent, permissionInterviewTask, talkAboutTask, type NeighborSubject } from "./neighborAgent";
import type { NeighborClient } from "./neighborClient";
import { onNeighborFocus, takeNeighborFocus } from "./neighborFocus";
import { neighborhoodNodes, STATUS_LABEL, type NeighborhoodNode } from "./neighborhoodModel";
import { neighborName } from "./neighborModel";
import { neighborRuntimeClient, type NeighborRuntimeClient, type ReceiverState } from "./neighborRuntimeClient";
import { neighborConciergeClient, type NeighborConciergeClient } from "./neighborConciergeClient";
import { referralStatus } from "./referralTrial";
import { useNeighbors } from "./useNeighbors";
import { useNeighborsSwitch } from "./neighborsSwitch";
import { readNeighborCache, writeNeighborCache, type CachedNeighbors } from "./neighborListCache";
import { ActivityDots } from "../shared/ActivityDots";
import { receivingLine, useReceiverState } from "./useReceiverState";
import { takeoverWarning } from "./NeighborChannel";

const openPlans = () => window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel: "settings:plans" } }));

function useTabMap() {
  const workspaces = useMuxSelector(state => state.workspaces);
  const tabs = useMuxSelector(state => state.tabs);
  return { workspaces, tabs };
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

export function NeighborsView({ config, client, runtimeClient = neighborRuntimeClient, conciergeClient = neighborConciergeClient }: {
  config?: object | null; client?: NeighborClient; runtimeClient?: NeighborRuntimeClient; conciergeClient?: NeighborConciergeClient;
}) {
  const neighborsSwitch = useNeighborsSwitch(config);
  const account = useAccount();
  const identity = account.model.kind === "signed_in" ? account.model.identity.uid : null;
  return <NeighborhoodPage key={identity} identity={identity} account={account} client={client} runtimeClient={runtimeClient} conciergeClient={conciergeClient}
    enabled={neighborsSwitch.enabled} onEnable={() => { void neighborsSwitch.setEnabled(true); }} />;
}

function NeighborhoodPage({ identity, account, client, runtimeClient, conciergeClient, enabled, onEnable }: {
  identity: string | null; account: ReturnType<typeof useAccount>; client?: NeighborClient; runtimeClient: NeighborRuntimeClient;
  conciergeClient: NeighborConciergeClient;
  enabled: boolean; onEnable: () => void;
}) {
  const neighbors = useNeighbors(identity, client);
  const tabMap = useTabMap();
  const me = accountTrayModel(account.model, null);
  const receiver = useReceiverState(runtimeClient, Boolean(identity) && enabled);
  const [selected, setSelected] = useState<string | null>(null);
  const [openSection, setOpenSection] = useState<"configure" | null>(null);
  const [adding, setAdding] = useState(false);
  const [agentError, setAgentError] = useState<string | null>(null);
  const awaitingAccept = useRef<string | null>(null);
  useEffect(() => {
    const take = () => { const focus = takeNeighborFocus(); if (focus) { setSelected(focus.id); setOpenSection(focus.section); } };
    take();
    return onNeighborFocus(take);
  }, []);
  const list = neighbors.list;
  // The last known graph draws at once; only a first load with nothing remembered shows a loading state.
  const cached = useMemo(() => readNeighborCache(identity), [identity]);
  const updating = identity !== null && list === null && cached !== null && !neighbors.error;
  const graph: CachedNeighbors | null = list ?? (updating ? cached : null);
  useEffect(() => { if (identity && list) writeNeighborCache(identity, list); }, [identity, list]);
  const paid = list?.access === "paid";
  const nodes = identity && enabled ? neighborhoodNodes(graph, graph !== null) : [];
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

  const people = (graph?.friends.length ?? 0);
  const requests = (graph?.incoming.length ?? 0);
  const invited = (graph?.outgoing.length ?? 0);
  const summary = [`${people} ${people === 1 ? "neighbor" : "neighbors"}`,
    ...(requests ? [`${requests} ${requests === 1 ? "request" : "requests"}`] : []), ...(invited ? [`${invited} invited`] : [])].join(", ");
  const panelOpen = enabled && Boolean(request);

  return <div className="neighbors-view absolute inset-0 flex flex-col bg-[var(--ct-bg)] pt-[var(--ct-safe-top,0px)]">
    <div className={`${UI_PANEL.toolbar} ct-section-toolbar gap-3 px-4 py-0 text-[var(--ct-fg)] whitespace-nowrap`}>
      <strong>Neighborhood</strong>
      <span className={`${UI_TEXT.meta} opacity-70`}>
        {identity && graph ? summary : null}
      </span>
      {updating && <span role="status" className="neighbors-updating"><ActivityDots size="small" tone="muted" active />Updating</span>}
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
          <div className="neighbor-me" data-signed-in={identity ? true : undefined} data-off={identity && !enabled ? true : undefined}>
            <span className="neighbor-me__ring">
              {me.kind === "signed_in" ? <AccountAvatar url={me.avatarUrl} initial={me.initial} label="" size="node" />
                : <span className="neighbor-me__ghost"><UserRound size={26} aria-hidden="true" /></span>}
            </span>
            {me.kind === "signed_in" && !enabled ? <div className="neighbor-me__signin" role="status">
                <p className="neighbor-me__title">Neighbors is off</p>
                <p className="neighbor-note">Your agents don't send or receive Neighbor messages. Your connections and their permissions are kept.</p>
                <Button size="sm" variant="primary" onClick={onEnable}>Turn on Neighbors</Button>
              </div>
              : me.kind === "signed_in" ? <><span className="neighbor-node__name">You</span>
                {receiver && <span role="status" className="neighbor-note neighbor-me__receiving">{receivingLine(receiver)}</span>}</>
              : <div className="neighbor-me__signin">
                <p className="neighbor-me__title">Sign in to meet your neighbors</p>
                <p className="neighbor-note">Your agents can message a friend's agents, end-to-end encrypted. Invited? Sign in with that email to start your 7-day trial, no card needed.</p>
                <Button size="sm" variant="primary" disabled={account.model.kind === "pending"} loading={account.model.kind === "pending"}
                  onClick={() => { void account.signIn(); }}>Sign in or create account</Button>
              </div>}
          </div>
          {nodes.map((node, index) => <NodeButton key={node.id} node={node} index={index} selected={node.id === selected}
            canInvite={graph?.can_invite} onSelect={() => {
              if (node.kind === "invited") return;
              if (node.kind === "add") {
                if (list?.can_invite) setAdding(true);
                else if (list?.can_invite === false) openPlans();
                return;
              }
              setOpenSection(null);
              setSelected(current => current === node.id ? null : node.id);
            }} />)}
          {identity && list === null && !updating && !neighbors.error && <div className="neighbors-loading"><BusyState inline label="Loading Neighbors" phase="work" /></div>}
        </div>
        {identity && enabled && list && people + requests + invited === 0 && <p className="neighbors-caption">Add someone you trust. Your agents can then ask theirs for help, and you decide what they may share.</p>}
        {identity && enabled && receiver && !receiver.receiving_here && <ReceiveHere receiver={receiver} client={runtimeClient} />}
        {identity && <NeighborPrivacyNote className="neighbors-privacy" />}
        {referral && <div role="status" className="ct-card ct-card--static neighbors-referral">
          <p className="ct-card__meta">{referral}</p>
          {neighbors.referral?.trial_end !== null && neighbors.referral?.trial_end !== undefined && <Button size="sm" variant="accent" onClick={openPlans}>Subscribe</Button>}
        </div>}
      </div>
      {enabled && request && <NeighborRequestPanel request={{ ...request, name: neighborName(request) }} pending={neighbors.pending}
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
    {enabled && friend && identity && <NeighborFriendPage key={friend.friendship_id} owner={identity} friend={friend} locked={!paid} enabled={enabled}
      pending={neighbors.pending} focusPermissions={openSection === "configure"} runtimeClient={runtimeClient} conciergeClient={conciergeClient}
      onBack={() => { setSelected(null); setOpenSection(null); }} onPlans={openPlans}
      onTalkAbout={() => ask(talkAboutTask({ name: neighborName(friend), friendshipId: friend.friendship_id }))}
      onInterview={() => interview({ name: neighborName(friend), friendshipId: friend.friendship_id })}
      onLimit={per_hour => neighbors.apply({ operation: "set_limit", friendship_id: friend.friendship_id, per_hour })}
      onRemove={() => neighbors.apply({ operation: "remove", friendship_id: friend.friendship_id })} />}
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
  if (node.kind === "invited") {
    const sent = node.createdAt * 1000;
    return <div className="neighbor-node neighbor-node--invited" style={style} data-status="invited" title={`Waiting for ${node.email} to accept. Sent ${new Date(sent).toLocaleString()}`}>
      <span className="neighbor-node__disc" aria-hidden="true"><Mail size={18} /></span>
      <span className="neighbor-node__name">{node.email}</span>
      <span className="neighbor-status"><span className="neighbor-dot" data-tone="muted" aria-hidden="true" />invited {formatAge(sent, Date.now())}</span>
    </div>;
  }
  return <button type="button" data-neighbor-node className="neighbor-node" style={style} data-status={node.status}
    aria-pressed={selected} aria-label={`${node.name}, ${STATUS_LABEL[node.status]}`} onClick={onSelect}>
    <span className="neighbor-node__disc"><AccountAvatar url={node.photoURL} initial={accountInitials(node.name)} label="" size="node" /></span>
    <span className="neighbor-node__name">{node.name}</span>
    <span className="neighbor-status"><span className="neighbor-dot" data-tone={node.status === "connected" ? "ok" : node.status === "request" ? "progress" : "muted"} aria-hidden="true" />{STATUS_LABEL[node.status]}</span>
  </button>;
}

/** Taking over receiving names who stops, and that concierges and permissions stay per machine. */
function ReceiveHere({ receiver, client }: { receiver: ReceiverState; client: NeighborRuntimeClient }) {
  const [confirming, setConfirming] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (confirming === null) {
    return <div className="neighbors-receive"><Button size="sm" onClick={() => { setError(null); setConfirming(receiver.revision); }}>Receive here</Button></div>;
  }
  const stale = confirming !== receiver.revision;
  return <div role="group" aria-label="Receive on this machine" className="ct-card ct-card--static neighbors-receive neighbors-receive--confirm">
    {stale && <p className="neighbor-note">Receiving changed. Confirm again to receive here.</p>}
    <p className="ct-card__meta">{takeoverWarning(receiver)}</p>
    {error && <InlineErrorNotice error={error} operation="receiving Neighbor messages here" role="alert" />}
    <div className="neighbor-confirm__actions">
      <Button size="sm" variant="primary" loading={busy} onClick={() => {
        if (stale) { setConfirming(receiver.revision); return; }
        setBusy(true); setError(null);
        void client.enroll(receiver.revision).then(() => setConfirming(null),
          (failure: unknown) => setError(failure instanceof Error ? failure.message : "Receiving could not move here.")).finally(() => setBusy(false));
      }}>Receive Neighbor messages here</Button>
      <Button size="sm" disabled={busy} onClick={() => setConfirming(null)}>Cancel</Button>
    </div>
  </div>;
}
