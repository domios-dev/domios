import { Fragment, useCallback, useEffect, useRef, useState } from "react";
import { Bot } from "lucide-react";
import { ChatDaySeparator, formatMessageTime, ReplyQuote, RichMarkdown } from "@codeterm/chat-ui";
import { AccountAvatar } from "../Titlebar/AccountAvatar";
import { accountInitials } from "../Settings/accountModel";
import { useAccount } from "../Settings/useAccount";
import { Button } from "../shared/Button";
import { DropdownMenu } from "../shared/DropdownMenu";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { RowMenuButton } from "../shared/RowMenuButton";
import { BusyState } from "../shared/BusyState";
import { UI_PANEL } from "../shared/design";
import { useMuxSelector } from "../../store/multiplexer";
import { formatUntil } from "../../lib/timeLabels";
import { askGeneralAgent, permissionInterviewTask } from "./neighborAgent";
import type { NeighborClient } from "./neighborClient";
import { neighborConciergeClient, type HistoryEntry, type NeighborConciergeClient } from "./neighborConciergeClient";
import { dayLabel, DELIVERY_LABEL, mergeHistory, speaker } from "./neighborConversationModel";
import { openNeighbor } from "./neighborFocus";
import { neighborName, type Neighbor } from "./neighborModel";
import { MissingConciergeNotice } from "./NeighborPanel";
import { NeighborComposer } from "./NeighborComposer";
import { useConciergeStatus } from "./useConciergeStatus";
import { neighborRuntimeClient, type NeighborRuntimeClient } from "./neighborRuntimeClient";
import { useNeighbors } from "./useNeighbors";
import { receivingLine, useReceiverState } from "./useReceiverState";

/** Your conversation with one Neighbor: what they asked, what you and your agents answered, and how it was delivered. */
export function NeighborConversationView({ friendship, enabled = true, client, runtimeClient = neighborRuntimeClient, conciergeClient = neighborConciergeClient }: {
  friendship: string; enabled?: boolean; client?: NeighborClient; runtimeClient?: NeighborRuntimeClient; conciergeClient?: NeighborConciergeClient;
}) {
  const { model } = useAccount();
  const identity = model.kind === "signed_in" ? model.identity.uid : null;
  const neighbors = useNeighbors(identity, client);
  const friend = neighbors.list?.friends.find(entry => entry.friendship_id === friendship);
  if (!friend) {
    return <div className="neighbor-chat absolute inset-0 flex items-center justify-center bg-[var(--ct-bg)]">
      {neighbors.list === null ? <BusyState inline label="Loading conversation" phase="work" />
        : <p className="neighbor-note">This Neighbor is no longer connected.</p>}
    </div>;
  }
  return <Conversation key={friendship} friend={friend} locked={neighbors.list?.access !== "paid"} enabled={enabled}
    runtimeClient={runtimeClient} conciergeClient={conciergeClient} onSent={neighbors.refresh} />;
}

function Conversation({ friend, locked, enabled, runtimeClient, conciergeClient, onSent }: {
  friend: Neighbor; locked: boolean; enabled: boolean; runtimeClient: NeighborRuntimeClient; conciergeClient: NeighborConciergeClient; onSent: () => void;
}) {
  const name = neighborName(friend);
  const friendship = friend.friendship_id;
  const workspaces = useMuxSelector(state => state.workspaces);
  const tabs = useMuxSelector(state => state.tabs);
  const receiver = useReceiverState(runtimeClient, enabled);
  const concierge = useConciergeStatus(runtimeClient, enabled)[friendship] ?? null;
  const setUpWithGeneral = () => {
    void askGeneralAgent({ workspaces, tabs }, permissionInterviewTask({ name, friendshipId: friendship }))
      .catch((failure: unknown) => setError(failure instanceof Error ? failure.message : "The General Agent could not be reached."));
  };
  const [entries, setEntries] = useState<HistoryEntry[] | null>(null);
  const [before, setBefore] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const scroller = useRef<HTMLDivElement>(null);
  const stickToEnd = useRef(true);
  // Only the first page sets the cursor for older messages; later refreshes only add new ones.
  const paged = useRef(false);

  const loadLatest = useCallback(async () => {
    try {
      const page = await conciergeClient.history(friendship);
      setEntries(current => mergeHistory(current ?? [], page.entries));
      if (!paged.current) { paged.current = true; setBefore(page.next_before); }
      setError(null);
    } catch (failure: unknown) { setError(failure instanceof Error ? failure.message : "The conversation could not be loaded."); }
  }, [conciergeClient, friendship]);

  useEffect(() => {
    void loadLatest();
    let stop: (() => void) | undefined;
    let live = true;
    void conciergeClient.onHistoryChanged(changed => { if (changed === friendship) void loadLatest(); })
      .then(unlisten => { if (live) stop = unlisten; else unlisten(); });
    return () => { live = false; stop?.(); };
  }, [conciergeClient, friendship, loadLatest]);

  useEffect(() => {
    const element = scroller.current;
    if (element && stickToEnd.current) element.scrollTop = element.scrollHeight;
  }, [entries]);

  const loadEarlier = async () => {
    if (!before) return;
    stickToEnd.current = false;
    try {
      const page = await conciergeClient.history(friendship, before);
      setEntries(current => mergeHistory(current ?? [], page.entries));
      setBefore(page.next_before);
    } catch (failure: unknown) { setError(failure instanceof Error ? failure.message : "Earlier messages could not be loaded."); }
  };

  const byId = new Map((entries ?? []).map(entry => [entry.message_id, entry]));
  const allowance = typeof friend.their_daily_limit === "number" ? { limit: friend.their_daily_limit, sent: friend.sent_today ?? 0 } : null;
  const limitReached = allowance !== null && allowance.sent >= allowance.limit;
  const canSend = enabled && !locked && !limitReached;
  const send = async (body: string): Promise<boolean> => {
    if (sending || !canSend) return false;
    setSending(true); setError(null); stickToEnd.current = true;
    try {
      await runtimeClient.send(friendship, body);
      onSent();
      await loadLatest();
      return true;
    } catch (failure: unknown) {
      setError(failure instanceof Error ? failure.message : "The message was not sent.");
      return false;
    } finally { setSending(false); }
  };

  return <div className="neighbor-chat absolute inset-0 flex flex-col bg-[var(--ct-bg)] pt-[var(--ct-safe-top,0px)]">
    <div className={`${UI_PANEL.toolbar} ct-section-toolbar gap-3 px-4 py-0 text-[var(--ct-fg)] whitespace-nowrap`}>
      <span className="neighbor-chat__avatar"><AccountAvatar url={friend.photoURL} initial={accountInitials(name)} label="" size="node" /></span>
      <strong className="min-w-0 truncate">{name}</strong>
      <span className="neighbor-status"><span className="neighbor-dot" data-tone={enabled && !locked ? "ok" : "muted"} aria-hidden="true" />
        {!enabled ? "Neighbors is off" : locked ? "paused by plan" : "connected"}</span>
      {allowance && <span className="neighbor-note" title={`${name} allows you ${allowance.limit} messages a day`}>{allowance.limit}/day, {allowance.sent} sent today</span>}
      <span className="ml-auto" />
      <DropdownMenu menuLabel={`${name} actions`} items={[
        { label: "View permissions", onClick: () => openNeighbor(friendship, "configure") },
        { label: "Edit with General", onClick: () => {
          void askGeneralAgent({ workspaces, tabs }, permissionInterviewTask({ name, friendshipId: friendship }))
            .catch((failure: unknown) => setError(failure instanceof Error ? failure.message : "The General Agent could not be reached."));
        } },
        { label: "Open in Neighborhood", onClick: () => openNeighbor(friendship) },
      ]} trigger={<RowMenuButton label={`Actions for ${name}`} title={`${name} actions`} />} />
    </div>
    {receiver && !receiver.receiving_here && receiver.receiving_enabled && <p role="status" className="neighbor-chat__moved">
      {receivingLine(receiver)}. {name}'s messages arrive there, and concierges here stay idle.</p>}
    {concierge?.concierge_missing && <div className="neighbor-chat__notice"><MissingConciergeNotice status={concierge}
      onSetUpWithGeneral={setUpWithGeneral} onEditPermissions={() => openNeighbor(friendship, "configure")} /></div>}
    <div ref={scroller} className="neighbor-chat__scroll" onScroll={event => {
      const element = event.currentTarget;
      stickToEnd.current = element.scrollHeight - element.scrollTop - element.clientHeight < 48;
    }}>
      <div className="neighbor-chat__column" role="log" aria-label={`Conversation with ${name}`}>
        {before && <div className="neighbor-chat__earlier"><Button size="sm" onClick={() => { void loadEarlier(); }}>Show earlier messages</Button></div>}
        {entries === null && !error && <BusyState inline label="Loading conversation" phase="work" />}
        {entries !== null && entries.length === 0 && <div className="neighbor-chat__empty">
          <p className="neighbor-me__title">No messages with {name} yet</p>
          <p className="neighbor-note">Their questions to your agents, and your answers, appear here.</p>
        </div>}
        {(entries ?? []).map((entry, index) => {
          const day = dayLabel(entry.at, index > 0 ? entries![index - 1].at : null);
          const quoted = entry.in_reply_to ? byId.get(entry.in_reply_to) : undefined;
          return <Fragment key={entry.message_id}>
            {day && <ChatDaySeparator label={day} />}
            <Entry entry={entry} friend={friend} friendName={name} quoted={quoted ? { label: speaker(quoted, name), excerpt: quoted.body } : null}
              onOpenQuoted={() => document.getElementById(`neighbor-msg-${entry.in_reply_to}`)?.scrollIntoView({ block: "center", behavior: "smooth" })} />
          </Fragment>;
        })}
      </div>
    </div>
    <div className="neighbor-chat__composer">
      {error && <InlineErrorNotice error={error} operation={`messaging ${name}`} role="alert" />}
      {limitReached && enabled && !locked && <p role="status" className="neighbor-note neighbor-chat__limit">Limit reached: {name} allows {allowance!.limit} {allowance!.limit === 1 ? "message" : "messages"} a day. Ask them to raise it.</p>}
      <NeighborComposer name={name} disabled={!canSend} sending={sending} onError={setError}
        placeholder={!enabled ? "Neighbors is off" : locked ? "Messages are paused by your plan" : limitReached ? "Daily limit reached" : `Message ${name}`}
        onSend={send} />
    </div>
  </div>;
}

function Entry({ entry, friend, friendName, quoted, onOpenQuoted }: {
  entry: HistoryEntry; friend: Neighbor; friendName: string; quoted: { label: string; excerpt: string } | null; onOpenQuoted: () => void;
}) {
  const time = formatMessageTime(entry.at);
  const mine = entry.direction === "out";
  const agent = typeof entry.author === "object" ? entry.author : null;
  return <div id={`neighbor-msg-${entry.message_id}`} className="neighbor-msg" data-side={mine ? "out" : "in"} data-kind={entry.kind}>
    {!mine && <span className="neighbor-msg__avatar"><AccountAvatar url={friend.photoURL} initial={accountInitials(friendName)} label="" size="node" /></span>}
    <div className="neighbor-msg__stack">
      {(agent || !mine) && <p className="neighbor-msg__who">{agent ? <><Bot size={11} aria-hidden="true" />{agent.name}</> : friendName}
        {entry.kind === "tab_request" && <span className="neighbor-note"> asked to open a tab</span>}
        {entry.kind === "refusal" && <span className="neighbor-note"> declined</span>}</p>}
      {quoted && <ReplyQuote label={quoted.label} excerpt={quoted.excerpt} onOpen={onOpenQuoted} />}
      <div className={mine ? `ct-user-bubble${agent ? " ct-user-bubble--relayed" : ""} neighbor-msg__bubble` : "neighbor-msg__body"}>
        <RichMarkdown text={entry.body} localArtifacts={false} />
      </div>
      <p className="neighbor-msg__meta">
        <time dateTime={new Date(entry.at).toISOString()} title={new Date(entry.at).toLocaleString()}>{time}</time>
        {mine && entry.reason === "neighbor_daily_limit" ? <span className="neighbor-status neighbor-msg__refused"><span className="neighbor-dot" data-tone="danger" aria-hidden="true" />
          Limit reached: {friendName} allows {entry.limit ?? "a few"} messages a day; ask them to raise it.{entry.resets_at ? ` Resets ${formatUntil(entry.resets_at, Date.now())}.` : ""}</span>
        : mine && entry.delivery_status && <span className="neighbor-status"><span className="neighbor-dot"
          data-tone={entry.delivery_status === "delivered" ? "ok" : entry.delivery_status === "pending" ? "progress" : "danger"} aria-hidden="true" />{DELIVERY_LABEL[entry.delivery_status]}</span>}
        {!mine && entry.outcome === "refused" && <span className="neighbor-status neighbor-msg__refused"><span className="neighbor-dot" data-tone="danger" aria-hidden="true" />refused{entry.reason ? `: ${entry.reason}` : ""}</span>}
        {!mine && entry.outcome === "answered" && <span className="neighbor-status"><span className="neighbor-dot" data-tone="ok" aria-hidden="true" />answered</span>}
      </p>
    </div>
  </div>;
}
