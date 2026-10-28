import { Fragment, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Bot, Check, CheckCheck, Flag, Paperclip } from "lucide-react";
import { ChatDaySeparator, formatMessageTime, ReplyQuote, RichMarkdown } from "@codeterm/chat-ui";
import { AccountAvatar } from "../Titlebar/AccountAvatar";
import { accountInitials } from "../Settings/accountModel";
import { useAccount } from "../Settings/useAccount";
import { Button } from "../shared/Button";
import { DropdownMenu } from "../shared/DropdownMenu";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { RowMenuButton } from "../shared/RowMenuButton";
import { BusyState } from "../shared/BusyState";
import { Loader } from "../shared/Loader";
import { ChatDeliveryState } from "../Terminal/ChatDeliveryState";
import { UI_PANEL } from "../shared/design";
import { ActivityDots } from "../shared/ActivityDots";
import { AgentIcon } from "../AgentIcon";
import { useMuxSelector } from "../../store/multiplexer";
import { tabActivity } from "../../lib/tabActivity";
import type { Tab } from "../../lib/types";
import { askGeneralAgent, permissionInterviewTask } from "./neighborAgent";
import type { NeighborClient } from "./neighborClient";
import { flagNeighborMessage, markNeighborRead, neighborConciergeClient, type HistoryEntry, type NeighborConciergeClient } from "./neighborConciergeClient";
import { flagLine, NeighborFlagDialog } from "./NeighborFlagDialog";
import { useNeighborReadReceipt } from "./useNeighborReadReceipt";
import { NOTIFY_CHOICES, useNeighborNotify } from "./neighborNotify";
import { pausedLine, setFriendPaused, theyPausedLine, usePausedFriends } from "./neighborPause";
import { activeWork, authorKind, CONCIERGE_WORK_LINE, conciergeWork, continuesGroup, dayLabel, landedSends, mergeHistory, readableBody, receiptMark, refusalLine, shownWork, speaker, type ConciergeWorkEntry, type OutgoingSend, type ReceiptMark } from "./neighborConversationModel";
import { openNeighborPage } from "./neighborFocus";
import { useNeighborGateway } from "./neighborGateways";
import { listedPaused, neighborName, type Neighbor } from "./neighborModel";
import { MissingConciergeNotice } from "./NeighborPanel";
import { NeighborComposer } from "./NeighborComposer";
import { useConciergeStatus } from "./useConciergeStatus";
import { neighborRuntimeClient, NeighborRuntimeFailure, type NeighborRuntimeClient } from "./neighborRuntimeClient";
import { useNeighbors } from "./useNeighbors";
import { readNeighborCache, type CachedFriend } from "./neighborListCache";
import { useNeighborPermission } from "./useNeighborPermission";
import { formatBytes, neighborFilesClient, NeighborFilesFailure, type NeighborFile, type NeighborFilesClient } from "./neighborFilesClient";
import { receivingLine, useReceiverState } from "./useReceiverState";

/** Your conversation with one Neighbor: what they asked, what you and your agents answered, and how it was delivered. */
export function NeighborConversationView({ friendship, enabled = true, client, runtimeClient = neighborRuntimeClient, conciergeClient = neighborConciergeClient, filesClient = neighborFilesClient, markRead = markNeighborRead, flagMessage = flagNeighborMessage }: {
  friendship: string; enabled?: boolean; client?: NeighborClient; runtimeClient?: NeighborRuntimeClient; conciergeClient?: NeighborConciergeClient; filesClient?: NeighborFilesClient;
  markRead?: (friendship: string, throughMessageId: string) => Promise<void>;
  flagMessage?: typeof flagNeighborMessage;
}) {
  const { model } = useAccount();
  const identity = model.kind === "signed_in" ? model.identity.uid : null;
  const neighbors = useNeighbors(identity, client);
  // The remembered friend opens the conversation at once; the fresh list fills in limits and status behind it.
  const cached = useMemo(() => readNeighborCache(identity), [identity]);
  const fresh = neighbors.list?.friends.find(entry => entry.friendship_id === friendship);
  const friend: ConversationFriend | undefined = fresh ?? (neighbors.list === null ? cached?.friends.find(entry => entry.friendship_id === friendship) : undefined);
  if (!friend && neighbors.list === null) {
    return <div className="neighbor-chat absolute inset-0 flex items-center justify-center bg-[var(--ct-bg)]">
      <BusyState inline label="Loading conversation" phase="work" />
    </div>;
  }
  // A removed friend keeps a readable history under the name we last knew them by.
  const removed = !friend;
  const shown: ConversationFriend = friend ?? cached?.friends.find(entry => entry.friendship_id === friendship)
    ?? { friendship_id: friendship, uid: friendship, displayName: "Former Neighbor", photoURL: null };
  const access = neighbors.list?.access ?? cached?.access;
  return <Conversation key={friendship} friend={shown} removed={removed} locked={access !== "paid"} enabled={enabled}
    runtimeClient={runtimeClient} conciergeClient={conciergeClient} filesClient={filesClient} markRead={markRead} flagMessage={flagMessage} onSent={neighbors.refresh} />;
}

/** A friend from the fresh list, or the remembered one while it loads. */
type ConversationFriend = CachedFriend & Partial<Omit<Neighbor, keyof CachedFriend>>;

const MESSAGE_LIMIT_BYTES = 32752;

function Conversation({ friend, removed, locked, enabled, runtimeClient, conciergeClient, filesClient, markRead, flagMessage, onSent }: {
  friend: ConversationFriend; removed: boolean; locked: boolean; enabled: boolean; runtimeClient: NeighborRuntimeClient; conciergeClient: NeighborConciergeClient;
  filesClient: NeighborFilesClient; markRead: (friendship: string, throughMessageId: string) => Promise<void>; flagMessage: typeof flagNeighborMessage; onSent: () => void;
}) {
  const name = neighborName(friend);
  const friendship = friend.friendship_id;
  const workspaces = useMuxSelector(state => state.workspaces);
  const tabs = useMuxSelector(state => state.tabs);
  const receiver = useReceiverState(runtimeClient, enabled);
  const { permission } = useNeighborPermission(runtimeClient, conciergeClient, friendship);
  // Files join their message by id; the list reloads whenever a transfer moves.
  const [files, setFiles] = useState<NeighborFile[]>([]);
  const filesGeneration = useRef(0);
  const reloadFiles = useCallback(() => {
    const current = ++filesGeneration.current;
    filesClient.list(friendship).then(value => { if (filesGeneration.current === current) setFiles(value.files); }, () => {});
  }, [filesClient, friendship]);
  useEffect(() => {
    reloadFiles();
    let stop: (() => void) | undefined;
    let live = true;
    void filesClient.onFilesChanged(changed => { if (changed === friendship) reloadFiles(); })
      .then(unlisten => { if (live) stop = unlisten; else unlisten(); });
    return () => { live = false; filesGeneration.current++; stop?.(); };
  }, [filesClient, friendship, reloadFiles]);
  // Their html and diagrams run only when the owner allowed interactive content from this friend.
  const friendInteractive = permission?.allow_interactive_content === true;
  const [checking, setChecking] = useState<string | null>(null);
  const [flagging, setFlagging] = useState<string | null>(null);
  const checkDelivery = (messageId: string) => {
    if (checking) return;
    setChecking(messageId); setError(null);
    void runtimeClient.deliveryStatus(messageId).then(() => loadLatest(),
      (failure: unknown) => setError(failure instanceof Error ? failure.message : "Delivery could not be checked."))
      .finally(() => setChecking(null));
  };
  const concierge = useConciergeStatus(runtimeClient, enabled)[friendship] ?? null;
  // The runtime's handling state says when the concierge works; until a runtime reports it, the bound tab's activity stands in.
  const tabStatuses = useMuxSelector(state => state.tabStatuses);
  const gatewayId = useNeighborGateway(friendship) ?? concierge?.gateway_tab ?? friend.gateway_tab ?? null;
  const conciergeTab: Tab | undefined = gatewayId ? tabs[gatewayId] : undefined;
  const tabWorking = enabled && conciergeTab !== undefined && tabActivity(conciergeTab, tabStatuses[conciergeTab.id]).kind === "working";
  const [stopping, setStopping] = useState(false);
  // Stopping goes through the runtime, so the entries it was handling come back as stopped.
  const stopConcierge = () => {
    if (stopping) return;
    setStopping(true);
    void conciergeClient.stopConcierge(friendship).then(() => loadLatest(),
      (failure: unknown) => setError(failure instanceof Error ? failure.message : "Your concierge could not be stopped."))
      .finally(() => setStopping(false));
  };
  const setUpWithGeneral = () => {
    void askGeneralAgent({ workspaces, tabs }, permissionInterviewTask({ name, friendshipId: friendship }))
      .catch((failure: unknown) => setError(failure instanceof Error ? failure.message : "The General Agent could not be reached."));
  };
  const [entries, setEntries] = useState<HistoryEntry[] | null>(null);
  // Sent messages show at once; each becomes its history entry when that lands, keeping its bubble.
  const [outgoing, setOutgoing] = useState<OutgoingSend[]>([]);
  const [before, setBefore] = useState<string | null>(null);
  // An ended friendship keeps its history readable but takes no new messages.
  const [historyEnded, setEnded] = useState(false);
  const ended = historyEnded || removed;
  const [error, setError] = useState<string | null>(null);
  // Until the first history read succeeds there is nothing to call empty: a loader, or its failure with a retry.
  const [historyFailure, setHistoryFailure] = useState<string | null>(null);
  const [notify, changeNotify] = useNeighborNotify(conciergeClient, friendship, friend.notify, setError);
  const [sending, setSending] = useState(false);
  const scroller = useRef<HTMLDivElement>(null);
  const stickToEnd = useRef(true);
  // Only the first page sets the cursor for older messages; later refreshes only add new ones.
  const paged = useRef(false);

  const loadLatest = useCallback(async () => {
    try {
      const page = await conciergeClient.history(friendship);
      setEntries(current => mergeHistory(current ?? [], page.entries));
      setEnded(page.ended);
      if (!paged.current) { paged.current = true; setBefore(page.next_before); }
      setError(null);
      setHistoryFailure(null);
    } catch (failure: unknown) {
      const message = failure instanceof Error ? failure.message : "The conversation could not be loaded.";
      if (paged.current) setError(message);
      else setHistoryFailure(message);
    }
  }, [conciergeClient, friendship]);
  const retryHistory = () => { setHistoryFailure(null); void loadLatest(); };

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
  }, [entries, outgoing]);
  useNeighborReadReceipt(scroller, entries, friendship, markRead);

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
  const bubbleKeys = useRef(new Map<string, string>());
  const landed = entries ? landedSends(outgoing, entries, new Set(bubbleKeys.current.values())) : new Map<string, string>();
  const bubbleKey = (messageId: string) => bubbleKeys.current.get(messageId)
    ?? [...landed].find(([, id]) => id === messageId)?.[0] ?? messageId;
  const pendingSends = outgoing.filter(item => !landed.has(item.key));
  const pendingByKey = new Map(pendingSends.map(item => [item.key, item]));
  const shown: HistoryEntry[] = [...(entries ?? []), ...pendingSends.map((item): HistoryEntry =>
    ({ direction: "out", kind: "question", author: "me", body: item.body, at: item.at, message_id: item.key }))];
  const work = conciergeWork(entries ?? [], { working: tabWorking, missing: enabled && concierge?.concierge_missing === true && !conciergeTab });
  const conciergeWorking = enabled && [...work.values()].some(entry => activeWork(entry.work));
  // The calm view of that standing: outcomes on their messages, the work under way once, "no reply needed" nowhere.
  const shownStanding = shownWork(work, (entries ?? []).map(entry => entry.message_id));
  const scrollTo = (messageId: string) => document.getElementById(`neighbor-msg-${messageId}`)?.scrollIntoView({ block: "center", behavior: "smooth" });
  // Your agents speak in your bubble; the caption names which one: the General Agent, or this friend's concierge.
  const agentLabel = (entry: HistoryEntry) => typeof entry.author === "object" && tabs[entry.author.agent]?.protected ? "General" : "Your concierge";
  const allowance = typeof friend.their_daily_limit === "number" ? { limit: friend.their_daily_limit, sent: friend.sent_today ?? 0 } : null;
  const limitReached = allowance !== null && allowance.sent >= allowance.limit;
  // Pausing is one way: your own pause leaves the composer open, theirs closes it.
  const paused = usePausedFriends(conciergeClient)(friendship, listedPaused(friend));
  const theyPaused = friend.their_paused === true;
  const canSend = enabled && !locked && !limitReached && !ended && !theyPaused;
  const sendCount = useRef(0);
  useEffect(() => {
    if (landed.size === 0) return;
    for (const [key, id] of landed) bubbleKeys.current.set(id, key);
    setOutgoing(current => current.filter(item => !landed.has(item.key)));
  });
  const settleSend = (key: string, change: Partial<OutgoingSend>) => setOutgoing(current => current.map(item => item.key === key ? { ...item, ...change } : item));
  const deliver = async (key: string, body: string) => {
    try {
      const receipt = await runtimeClient.send(friendship, body);
      settleSend(key, { messageId: receipt.message_id });
      onSent();
    } catch (failure: unknown) {
      // Only a refusal the runtime named proves nothing went out; anything else may have, so it stays sending and is never sent again.
      if (failure instanceof NeighborRuntimeFailure && (failure.reason !== "unavailable" || failure.refusal)) {
        const refused = failure.refusal;
        const resetsAt = refused?.resets_at === undefined ? undefined : refused.resets_at < 1e12 ? refused.resets_at * 1000 : refused.resets_at;
        const line = refused ? refusalLine({ direction: "out", kind: "question", author: "me", body: "", at: Date.now(), message_id: "", reason: refused.reason,
          limit: refused.limit, resets_at: resetsAt, retry_after_seconds: refused.retry_after_seconds }, name, Date.now()) : null;
        settleSend(key, { state: "failed", reason: line ?? failure.message });
      } else settleSend(key, { uncertain: true });
    }
    await loadLatest();
  };
  const send = async (body: string): Promise<boolean> => {
    if (!canSend) return false;
    // The encrypted envelope carries at most 32752 UTF-8 bytes of text; the draft stays for trimming.
    if (new TextEncoder().encode(body).length > MESSAGE_LIMIT_BYTES) {
      setError(`This message is too long for ${name}. Keep it under 32 KB.`);
      return false;
    }
    const key = `outgoing-${++sendCount.current}`;
    setError(null); stickToEnd.current = true;
    setOutgoing(current => [...current, { key, body, at: Date.now(), state: "sending" }]);
    void deliver(key, body);
    return true;
  };
  // Retrying is the owner's choice and only follows a proven refusal, so the failed bubble gives way to a fresh send.
  const retrySend = (item: OutgoingSend) => {
    setOutgoing(current => current.filter(entry => entry.key !== item.key));
    void send(item.body);
  };
  const sendFiles = async (body: string, paths: string[]): Promise<boolean> => {
    if (sending || !canSend) return false;
    setSending(true); setError(null); stickToEnd.current = true;
    try {
      await filesClient.send(friendship, body, paths);
      onSent();
      reloadFiles();
      await loadLatest();
      return true;
    } catch (failure: unknown) {
      setError(fileRefusal(failure, name));
      return false;
    } finally { setSending(false); }
  };

  return <div className="neighbor-chat absolute inset-0 flex flex-col bg-[var(--ct-bg)] pt-[var(--ct-safe-top,0px)]">
    <div className={`${UI_PANEL.toolbar} ct-section-toolbar gap-3 px-4 py-0 text-[var(--ct-fg)] whitespace-nowrap`}>
      <button type="button" className="neighbor-chat__who ct-liquid-interactive" aria-label={`Open ${name}'s page`} onClick={() => openNeighborPage(friendship)}>
        <span className="neighbor-chat__avatar"><AccountAvatar url={friend.photoURL} initial={accountInitials(name)} label="" size="node" /></span>
        <strong className="min-w-0 truncate">{name}</strong>
      </button>
      <span className="neighbor-status"><span className="neighbor-dot" data-tone={enabled && !locked && !removed ? "ok" : "muted"} aria-hidden="true" />
        {removed ? "no longer connected" : !enabled ? "Neighbors is off" : locked ? "paused by plan" : "connected"}</span>
      {allowance && <span className="neighbor-note" title={`${name} allows you ${allowance.limit} messages a day`}>{allowance.limit}/day, {allowance.sent} sent today</span>}
      <span className="ml-auto" />
      <DropdownMenu menuLabel={`${name} actions`} items={[
        { label: "Notifications", submenu: { label: "Notifications", items: NOTIFY_CHOICES.map(choice => ({
          label: choice.label, selected: notify === choice.value, onClick: () => changeNotify(choice.value) })) } },
        { label: "View permissions", onClick: () => openNeighborPage(friendship, "permissions") },
        { label: "Edit with General", onClick: () => {
          void askGeneralAgent({ workspaces, tabs }, permissionInterviewTask({ name, friendshipId: friendship }))
            .catch((failure: unknown) => setError(failure instanceof Error ? failure.message : "The General Agent could not be reached."));
        } },
        { label: "Open in Neighborhood", onClick: () => openNeighborPage(friendship) },
      ]} trigger={<RowMenuButton label={`Actions for ${name}`} title={`${name} actions`} />} />
    </div>
    {receiver && !receiver.receiving_here && receiver.receiving_enabled && <p role="status" className="neighbor-chat__moved">
      {receivingLine(receiver)}. {name}'s messages arrive there, and concierges here stay idle.</p>}
    {concierge?.concierge_missing && !concierge.gateway_tab && !friend.gateway_tab && <div className="neighbor-chat__notice"><MissingConciergeNotice status={concierge}
      onSetUpWithGeneral={setUpWithGeneral} onEditPermissions={() => openNeighborPage(friendship, "permissions")} /></div>}
    <div ref={scroller} className="neighbor-chat__scroll" onScroll={event => {
      const element = event.currentTarget;
      stickToEnd.current = element.scrollHeight - element.scrollTop - element.clientHeight < 48;
    }}>
      <div className="neighbor-chat__column" role="log" aria-label={`Conversation with ${name}`}>
        {before && <div className="neighbor-chat__earlier"><Button size="sm" onClick={() => { void loadEarlier(); }}>Show earlier messages</Button></div>}
        {entries === null && !historyFailure && <Loader label="Loading the conversation" />}
        {entries === null && historyFailure && <InlineErrorNotice error={historyFailure} operation={`loading your conversation with ${name}`}
          actions={[{ label: "Retry", onClick: retryHistory }]} />}
        {entries !== null && entries.length === 0 && pendingSends.length === 0 && !error && <div className="neighbor-chat__empty">
          <p className="neighbor-me__title">{removed ? "No longer connected" : "No messages yet"}</p>
          <p className="neighbor-me__hint">{removed ? `You and ${name} are no longer Neighbors.` : `Say hi to ${name}. Your concierge answers their agents within the permissions you set.`}</p>
        </div>}
        {shown.map((entry, index) => {
          const day = dayLabel(entry.at, index > 0 ? shown[index - 1].at : null);
          const pending = pendingByKey.get(entry.message_id);
          const quoted = entry.in_reply_to ? byId.get(entry.in_reply_to) : undefined;
          const filesOf = (item: HistoryEntry) => files.filter(file => file.message_id === item.message_id);
          return <Fragment key={bubbleKey(entry.message_id)}>
            {day && <ChatDaySeparator label={day} />}
            <Entry entry={entry} friend={friend} friendName={name} agentLabel={agentLabel(entry)}
              files={filesOf(entry)}
              onOpenFile={file => { void filesClient.open(friendship, file.file_id).catch((failure: unknown) => setError(failure instanceof Error ? failure.message : "The file could not be opened.")); }} grouped={!day && continuesGroup(entry, index > 0 ? shown[index - 1] : null)} interactive={friendInteractive && entry.direction === "in" && entry.author === "friend"}
              checking={checking === entry.message_id} onCheckDelivery={() => checkDelivery(entry.message_id)} quoted={quoted ? { label: speaker(quoted, name), excerpt: readableBody(quoted.body, new Set(filesOf(quoted).map(file => file.file_id))) } : null}
              onOpenQuoted={() => scrollTo(entry.in_reply_to!)}
              standing={shownStanding.has(entry.message_id) && <ConciergeWorkLine work={shownStanding.get(entry.message_id)!} tab={conciergeTab}
                canStop={enabled} stopping={stopping} onStop={stopConcierge} onOpenReply={scrollTo} />}
              settled={shownStanding.has(entry.message_id) && settledWork(shownStanding.get(entry.message_id)!.work)}
              quietOutcome={work.get(entry.message_id)?.work === "no_reply" ? CONCIERGE_WORK_LINE.no_reply : undefined}
              outgoing={pending && { state: pending.state, reason: pending.reason, onRetry: () => retrySend(pending) }}
              onFlag={entry.direction === "in" && !pending ? () => setFlagging(entry.message_id) : undefined} />
          </Fragment>;
        })}
      </div>
    </div>
    <div className="neighbor-chat__composer">
      {error && <InlineErrorNotice error={error} operation={`messaging ${name}`} role="alert" />}
      {ended && <p role="status" className="neighbor-note neighbor-chat__limit">This connection with {name} has ended. The conversation stays here for you to read.</p>}
      {!ended && limitReached && enabled && !locked && <p role="status" className="neighbor-note neighbor-chat__limit">Limit reached: {name} allows {allowance!.limit} {allowance!.limit === 1 ? "message" : "messages"} a day. Ask them to raise it.</p>}
      {!ended && paused && <div role="status" className="neighbor-note neighbor-chat__limit neighbor-chat__paused">
        <span>{pausedLine(name)}</span>
        <Button size="sm" onClick={() => setFriendPaused(conciergeClient, friendship, false, true, setError)}>Enable</Button>
      </div>}
      {!ended && theyPaused && <p role="status" className="neighbor-note neighbor-chat__limit">{theyPausedLine(name)}</p>}
      {!ended && conciergeWorking && <div role="status" className="neighbor-note neighbor-chat__limit neighbor-chat__working">
        <ConciergeIcon tab={conciergeTab} />
        <span>Your concierge is working for {name}</span>
        <ActivityDots size="small" tone="muted" />
        <Button size="compact" className="ml-auto" loading={stopping} aria-label="Stop your concierge" onClick={stopConcierge}>Stop</Button>
      </div>}
      <NeighborComposer name={name} disabled={!canSend} sending={sending} onError={setError}
        placeholder={ended ? "This connection has ended" : !enabled ? "Neighbors is off" : locked ? "Messages are paused by your plan" : theyPaused ? `${name} disabled messages from you`
          : limitReached ? "Daily limit reached" : `Message ${name}`}
        onSend={send} onSendFiles={sendFiles} />
    </div>
    {flagging && <NeighborFlagDialog name={name} onClose={() => setFlagging(null)}
      onFlag={(category, reason) => flagMessage(friendship, flagging, category, reason).then(() => loadLatest())} />}
  </div>;
}

function Entry({ entry, friend, friendName, agentLabel, files, onOpenFile, grouped, interactive, checking, onCheckDelivery, quoted, onOpenQuoted, standing, settled, outgoing, onFlag, quietOutcome }: {
  entry: HistoryEntry; friend: ConversationFriend; friendName: string; agentLabel: string; files: NeighborFile[]; onOpenFile: (file: NeighborFile) => void; grouped: boolean; interactive: boolean; checking: boolean; onCheckDelivery: () => void;
  quoted: { label: string; excerpt: string } | null; onOpenQuoted: () => void;
  /** Your concierge's standing on this message: inside the time row once settled, its own line while under way. */
  standing?: ReactNode; settled?: boolean;
  /** A send history has not shown yet: the shared delivery dot, or the refusal that proved it never went out. */
  outgoing?: { state: "sending" | "failed"; reason?: string; onRetry: () => void };
  /** Messages from them can be flagged by you from their menu. */
  onFlag?: () => void;
  /** An outcome too quiet for a line of its own ("No reply needed"), kept in the time's tooltip. */
  quietOutcome?: string;
}) {
  const time = formatMessageTime(entry.at);
  const author = authorKind(entry);
  const mine = entry.direction === "out";
  const refusal = outgoing?.state === "failed" ? outgoing.reason ?? "Not sent." : mine ? refusalLine(entry, friendName, Date.now()) : null;
  const refused = entry.kind === "refusal" || entry.outcome === "refused" || refusal !== null;
  // Sending until the relay accepted it, then one tick, two once delivered, accent once read.
  const mark = receiptMark(entry);
  const flagged = !mine && entry.flags?.length ? flagLine(entry.flags) : null;
  const agentName = typeof entry.author === "object" ? agentLabel : `${friendName}'s concierge`;
  const avatar = grouped ? <span className="neighbor-msg__avatar" aria-hidden="true" />
    : author === "me" || author === "my-agent" ? null
    : <span className="neighbor-msg__avatar" title={author === "friend-agent" ? agentName : friendName}>
      <AccountAvatar url={friend.photoURL} initial={accountInitials(friendName)} label="" size="node" />
      {author === "friend-agent" && <span className="neighbor-msg__badge"><Bot size={8} aria-hidden="true" /></span>}
    </span>;
  return <div id={`neighbor-msg-${entry.message_id}`} className="neighbor-msg" data-message-id={entry.message_id} data-side={mine ? "out" : "in"} data-author={author}
    data-kind={entry.kind} data-grouped={grouped || undefined} data-refused={refused || undefined}
    aria-label={author === "me" ? "You" : author === "my-agent" ? agentName : author === "friend-agent" ? agentName : friendName}>
    {!mine && avatar}
    <div className="neighbor-msg__stack">
      {author === "my-agent" && !grouped && <p className="neighbor-msg__caption">{agentLabel}</p>}
      {entry.kind === "tab_request" && <p className="neighbor-note">asked to open a tab</p>}
      {/* The message's own menu sits beside its bubble, shown on hover, as in other messengers. */}
      <div className="neighbor-msg__bubble-row">
        <div className={mine ? "ct-user-bubble neighbor-msg__bubble" : "neighbor-msg__bubble"}>
          {quoted && <ReplyQuote label={quoted.label} excerpt={quoted.excerpt} onOpen={onOpenQuoted} />}
          <RichMarkdown text={readableBody(entry.body, new Set(files.map(file => file.file_id)))} localArtifacts={false} interactivePreviews={interactive} />
          {outgoing ? <ChatDeliveryState state={outgoing.state} onRetry={outgoing.onRetry} /> : mark === "sending" && <ChatDeliveryState state="sending" />}
        </div>
        {onFlag && <DropdownMenu className="neighbor-msg__menu" menuLabel="Message actions" items={[{ label: "Flag message…", onClick: onFlag }]}
          trigger={<RowMenuButton label={`Actions for this message from ${friendName}`} title="Message actions" />} />}
      </div>
      {files.length > 0 && <div className="neighbor-files" role="list" aria-label="Files">{files.map(file => <FileChip key={file.file_id} file={file} onOpen={onOpenFile} />)}</div>}
      <p className="neighbor-msg__meta">
        <time dateTime={new Date(entry.at).toISOString()} title={`${new Date(entry.at).toLocaleString()}${quietOutcome ? `. ${quietOutcome}` : ""}`}>{time}</time>
        {mine && (refusal || mark === "not_delivered") ? <span className="neighbor-work neighbor-work--inline" data-work="refused">
          <span className="neighbor-work__text" title={refusal ?? undefined}>{refusal ?? "Not delivered"}</span></span>
        : mark && mark !== "sending" && mark !== "not_delivered" && <ReceiptTicks mark={mark} at={mark === "read" ? entry.read_at! : mark === "delivered" ? entry.delivered_at ?? null : entry.at} />}
        {mine && entry.submission === "unconfirmed" && <span className="neighbor-note">submission unconfirmed</span>}
        {mine && !refusal && entry.delivery_status === "pending" && <Button size="compact" loading={checking} onClick={onCheckDelivery}>Check delivery</Button>}
        {settled && standing}
        {flagged && <span className="neighbor-work neighbor-work--inline" data-work="flagged">
          <Flag size={11} aria-hidden="true" /><span className="neighbor-work__text" title={flagged.detail}>{flagged.text}</span></span>}
      </p>
      {!settled && standing}
    </div>
  </div>;
}

const RECEIPT_LABEL: Record<Exclude<ReceiptMark, "sending" | "not_delivered">, string> = { sent: "Sent", delivered: "Delivered", read: "Read" };

/** One grey tick once sent, two once delivered, two in the accent once read; the tooltip says when. */
function ReceiptTicks({ mark, at }: { mark: Exclude<ReceiptMark, "sending" | "not_delivered">; at: number | null }) {
  const label = RECEIPT_LABEL[mark];
  const Icon = mark === "sent" ? Check : CheckCheck;
  return <span role="img" className="neighbor-receipt" data-mark={mark} aria-label={label}
    title={at === null ? label : `${label} ${new Date(at).toLocaleString()}`}>
    <Icon size={13} strokeWidth={2.25} aria-hidden="true" />
  </span>;
}

function ConciergeIcon({ tab }: { tab: Tab | undefined }) {
  return <span className="neighbor-work__icon" aria-hidden="true">
    {tab?.agent_type ? <AgentIcon providerId={tab.agent_type} className="text-[12px]" /> : <Bot size={12} />}</span>;
}

/** A settled outcome joins the message's time; work still under way keeps its own line with Stop. */
const settledWork = (work: ConciergeWorkEntry["work"]) => !activeWork(work) && work !== "waiting";

/** Where your concierge stands on a friend's message: settled, a quiet part of the time row; under way, one line under it that Stop interrupts. */
function ConciergeWorkLine({ work, tab, canStop, stopping, onStop, onOpenReply }: {
  work: ConciergeWorkEntry; tab: Tab | undefined; canStop: boolean; stopping: boolean; onStop: () => void; onOpenReply: (messageId: string) => void;
}) {
  const line = CONCIERGE_WORK_LINE[work.work];
  const active = activeWork(work.work);
  const settled = settledWork(work.work);
  // An answer or a refusal links to the reply that already says it all; without a reply, the reason is one truncated line.
  const reply = (work.work === "answered" || work.work === "refused") && work.reply;
  const reason = !reply && work.reason ? work.reason : null;
  const content = <>
    <ConciergeIcon tab={tab} />
    {reply
      ? <button type="button" className="neighbor-work__reply ct-liquid-interactive" onClick={() => onOpenReply(reply)}>{line}</button>
      : <span className="neighbor-work__text" title={reason ?? undefined}>{line}{reason ? `: ${reason}` : ""}</span>}
    {work.work === "working" && <ActivityDots size="small" tone="muted" />}
    {active && canStop && <Button size="compact" loading={stopping} aria-label="Stop your concierge" onClick={onStop}>Stop</Button>}
  </>;
  return settled
    ? <span className="neighbor-work neighbor-work--inline" data-work={work.work}>{content}</span>
    : <div className="neighbor-work" data-work={work.work} role="status">{content}</div>;
}

const FILE_STATE: Record<NeighborFile["state"], { label: string; tone: "ok" | "progress" | "muted" | "danger" }> = {
  saved: { label: "saved", tone: "ok" },
  downloading: { label: "downloading", tone: "progress" },
  expired: { label: "expired", tone: "muted" },
  failed: { label: "failed", tone: "danger" },
};

/** One attached file: its name, size and where the transfer stands; a saved file from them opens. */
function FileChip({ file, onOpen }: { file: NeighborFile; onOpen: (file: NeighborFile) => void }) {
  const state = FILE_STATE[file.state];
  const content = <>
    <Paperclip size={12} aria-hidden="true" />
    <span className="neighbor-file__name">{file.name}</span>
    <span className="neighbor-note">{formatBytes(file.size_bytes)}</span>
    <span className="neighbor-status"><span className="neighbor-dot" data-tone={state.tone} aria-hidden="true" />{state.label}</span>
  </>;
  const openable = file.direction === "in" && file.state === "saved";
  // The list item wraps the button, so the chip stays a button that assistive tech and automation can press.
  return openable
    ? <span role="listitem" className="neighbor-file__item">
      <button type="button" className="neighbor-file ct-liquid-interactive" aria-label={`Open ${file.name}`} onClick={() => onOpen(file)}>{content}</button>
    </span>
    : <span role="listitem" className="neighbor-file" data-state={file.state}>{content}</span>;
}

/** What a refused file send means, in the words the owner needs. */
function fileRefusal(failure: unknown, name: string): string {
  if (!(failure instanceof NeighborFilesFailure)) return failure instanceof Error ? failure.message : "The files were not sent.";
  switch (failure.reason) {
    case "file_too_large": return `Files can be up to ${formatBytes(failure.limitBytes ?? 2 * 1024 * 1024)} each.`;
    case "file_daily_limit": return `${name} accepts no more files today.`;
    case "sender_quota": return "You have sent all the files you can today.";
    case "files_disabled": return `${name} does not accept files.`;
    case "recipient_files_unsupported": return `${name}'s Domios cannot receive files yet.`;
    case "file_rate_limited": return "Too many files right now. Try again shortly.";
    case "recipient_paused": return `Not sent: ${name} disabled messages from you.`;
    default: return failure.message;
  }
}
