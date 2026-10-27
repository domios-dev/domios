import { useEffect, useId, useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { Bell, Bot, FolderOpen, MessagesSquare, ShieldCheck, Sparkles, UserX, X } from "lucide-react";
import { AccountAvatar } from "../Titlebar/AccountAvatar";
import { accountInitials } from "../Settings/accountModel";
import { SettingsCard, SettingsPage, SettingsPill } from "../Settings/shared";
import { Button } from "../shared/Button";
import { DropdownSelect } from "../shared/DropdownMenu";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useBackDismissal } from "../../lib/backDismissal";
import { trapDialogTab, useDialogFocus } from "../shared/dialogFocusTrap";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { useMuxSelector } from "../../store/multiplexer";
import { ChangeConciergeDialog } from "./ChangeConciergeDialog";
import { focusTab, workspaceOfTab } from "./neighborAgent";
import type { HistoryEntry, NeighborConciergeClient, NeighborNotify } from "./neighborConciergeClient";
import { Chip } from "../shared/Chip";
import { openNeighbor, type NeighborSection } from "./neighborFocus";
import { neighborName, type Neighbor } from "./neighborModel";
import { MissingConciergeNotice } from "./NeighborPanel";
import { NeighborPermissionsEditor } from "./NeighborPermissionsEditor";
import { formatBytes, neighborFilesClient, type NeighborFileQuota, type NeighborFilesClient } from "./neighborFilesClient";
import { formatAge, formatUntil } from "../../lib/timeLabels";
import type { NeighborPermission, NeighborRuntimeClient } from "./neighborRuntimeClient";
import { useConciergeStatus, type ConciergeStatus } from "./useConciergeStatus";
import { useNeighborPermission } from "./useNeighborPermission";
import { receivingLine, useReceiverState } from "./useReceiverState";

export const NOTIFY_CHOICES: { value: NeighborNotify; label: string }[] = [
  { value: "off", label: "Off" },
  { value: "in_app", label: "In Domios" },
  { value: "in_app_and_bridges", label: "In Domios and your bridges" },
];

function conciergeState(status: ConciergeStatus | null, bound: boolean): { label: string; tone: "green" | "amber" | "muted" | "accent" } {
  if (status?.state === "concierge_missing" || (!status && !bound)) return { label: "missing", tone: "amber" };
  if (status?.state === "concierge_starting") return { label: "starting", tone: "accent" };
  if (status?.state === "receiving_elsewhere") return { label: "idle", tone: "muted" };
  if (status?.state === "not_receiving") return { label: "not receiving", tone: "muted" };
  return { label: "ready", tone: "green" };
}

/** One Neighbor's settings: the conversation, their concierge, what they may ask, notifications, and removal. */
export function NeighborFriendPage({ friend, locked, enabled, pending, section, origin, runtimeClient, conciergeClient, filesClient = neighborFilesClient,
  onBack, onTalkAbout, onInterview, onLimit, onRemove, onPlans }: {
  friend: Neighbor; locked: boolean; enabled: boolean; pending: boolean;
  /** The section the window opens scrolled to, when an entry point asked for one. */
  section: NeighborSection | null;
  /** Viewport centre of the node that opened this window, so it can grow from and fold back into it. */
  origin?: { x: number; y: number } | null;
  runtimeClient: NeighborRuntimeClient; conciergeClient: NeighborConciergeClient; filesClient?: NeighborFilesClient;
  onBack: () => void; onTalkAbout: () => void; onInterview: () => void;
  onLimit: (limit: number) => Promise<boolean>; onRemove: () => Promise<boolean>; onPlans: () => void;
}) {
  const name = neighborName(friend);
  const friendship = friend.friendship_id;
  const { permission } = useNeighborPermission(runtimeClient, conciergeClient, friendship);
  const receiver = useReceiverState(runtimeClient, enabled);
  const concierge = useConciergeStatus(runtimeClient, enabled)[friendship] ?? null;
  const workspaces = useMuxSelector(state => state.workspaces);
  const tabs = useMuxSelector(state => state.tabs);
  const { providers } = useAgentProviders();
  const [changing, setChanging] = useState(false);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [notify, setNotify] = useState<NeighborNotify>(friend.notify ?? "in_app");
  const [error, setError] = useState<string | null>(null);
  const [quota, setQuota] = useState<NeighborFileQuota | null>(null);
  // The newest page of the conversation is enough to count today's messages from them and date the last one.
  const [recent, setRecent] = useState<HistoryEntry[] | null>(null);
  useEffect(() => {
    const controller = new AbortController();
    conciergeClient.history(friendship, undefined, controller.signal).then(page => setRecent(page.entries), () => setRecent(null));
    return () => controller.abort();
  }, [conciergeClient, friendship]);
  useEffect(() => {
    const controller = new AbortController();
    filesClient.usage(friendship, controller.signal).then(setQuota, () => setQuota(null));
    return () => controller.abort();
  }, [filesClient, friendship]);
  const permissions = useRef<HTMLDivElement>(null);
  const gatewayId = concierge?.gateway_tab ?? friend.gateway_tab ?? null;
  const gateway = gatewayId ? tabs[gatewayId] : undefined;
  const state = conciergeState(concierge, Boolean(gateway));
  const provider = gateway?.agent_type ? providers.find(entry => entry.id === gateway.agent_type) : undefined;
  const model = gateway?.agent_provider_resolved_model;

  useEffect(() => { if (friend.notify) setNotify(friend.notify); }, [friend.notify]);
  useEffect(() => {
    let stop: (() => void) | undefined;
    let live = true;
    void conciergeClient.onNotifyChanged((changed, value) => { if (changed === friendship) setNotify(value); })
      .then(unlisten => { if (live) stop = unlisten; else unlisten(); });
    return () => { live = false; stop?.(); };
  }, [conciergeClient, friendship]);
  const conciergeCard = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const target = section === "permissions" ? permissions.current : section === "concierge" ? conciergeCard.current : null;
    target?.scrollIntoView({ block: "start" });
  }, [section]);

  const fail = (failure: unknown, fallback: string) => setError(failure instanceof Error ? failure.message : fallback);

  // The window grows out of the node that was clicked and folds back into it on close.
  const titleId = useId();
  const layer = useRef<HTMLDivElement>(null);
  // Focus starts inside the window and returns to this friend's node, wherever the window was opened from.
  const windowRef = useDialogFocus<HTMLElement>(() => document.querySelector<HTMLElement>(`[data-node-id="${friendship}"]`));
  const [closing, setClosing] = useState(false);
  const [fromStyle, setFromStyle] = useState<CSSProperties | undefined>(undefined);
  useLayoutEffect(() => {
    const box = layer.current?.getBoundingClientRect();
    if (box && origin) {
      setFromStyle({ "--neighbor-from-dx": `${origin.x - box.left - box.width / 2}px`, "--neighbor-from-dy": `${origin.y - box.top - box.height / 2}px` } as CSSProperties);
    }
  }, [origin]);
  const close = () => {
    if (closing) return;
    // Without motion there is no animation end to wait for, so the window closes at once.
    const still = typeof window.matchMedia !== "function" || window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (still) onBack();
    else setClosing(true);
  };
  useBackDismissal(true, close);
  const status = !enabled ? "Neighbors is off" : locked ? "paused by plan" : "connected";

  return <div ref={layer} className="neighbor-window-layer" data-closing={closing || undefined}
    onMouseDown={event => { if (event.target === event.currentTarget) close(); }}>
    <section ref={windowRef} role="dialog" aria-modal="true" aria-labelledby={titleId} tabIndex={-1} className="neighbor-window" style={fromStyle}
      onKeyDown={event => {
        // The concierge dialog above owns the keyboard while it is open.
        if (changing) return;
        if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); close(); return; }
        trapDialogTab(event, windowRef.current);
      }}
      onAnimationEnd={event => { if (closing && event.target === event.currentTarget) onBack(); }}>
      <header className="neighbor-window__hero">
        <span className="neighbor-window__avatar"><AccountAvatar url={friend.photoURL} initial={accountInitials(name)} label="" size="node" /></span>
        <div className="min-w-0 flex-1">
          <h2 id={titleId} className="neighbor-friend__name">{name}</h2>
          <p className="neighbor-status"><span className="neighbor-dot" data-tone={enabled && !locked ? "ok" : "muted"} aria-hidden="true" />{status}
            <span className="neighbor-note">, since {new Date(friend.since * 1000).toLocaleDateString()}</span></p>
          {receiver && <p className="neighbor-note">{receivingLine(receiver)}</p>}
          <FriendStats friend={friend} quota={quota} history={recent} />
        </div>
        <div className="neighbor-window__actions">
          {locked ? <Button size="sm" variant="accent" onClick={onPlans}>View plans</Button>
            : <Button size="sm" variant="primary" onClick={() => openNeighbor(friendship)}><MessagesSquare size={14} aria-hidden="true" />Open conversation</Button>}
          <Button size="sm" onClick={onTalkAbout}><Sparkles size={14} aria-hidden="true" />Talk about {name}</Button>
          <button type="button" className="neighbor-window__close ct-liquid-interactive" aria-label={`Close ${name}`} onClick={close}><X size={15} aria-hidden="true" /></button>
        </div>
      </header>
      <div className="neighbor-window__body">
      <SettingsPage title={name}>
        {error && <InlineErrorNotice error={error} operation={`managing ${name}`} role="alert" />}
        {concierge?.concierge_missing && !gateway && <MissingConciergeNotice status={concierge} onSetUpWithGeneral={onInterview}
          onEditPermissions={() => permissions.current?.scrollIntoView({ block: "start", behavior: "smooth" })} />}

        <div ref={conciergeCard}>
        <SettingsCard icon={<Bot size={16} />} title="Concierge" description={`The agent that answers ${name} within the permissions below.`}
          headerRight={<SettingsPill tone={state.tone}>{state.label}</SettingsPill>}>
          <div className="neighbor-friend__row">
            <span className="min-w-0">
              <span className="neighbor-friend__label">{gateway ? `${provider?.display_name ?? gateway.agent_type ?? "Agent"}${model ? `, ${model}` : ""}` : "No concierge on this machine"}</span>
              {gateway && <span className="neighbor-note"> in {gateway.title}</span>}
            </span>
            <span className="neighbor-confirm__actions">
              {gateway && gatewayId && <Button size="sm" onClick={() => focusTab(gatewayId, workspaceOfTab({ workspaces, tabs }, gatewayId))}>Open its chat</Button>}
              <Button size="sm" variant={gateway ? "ghost" : "accent"} onClick={() => setChanging(true)}>{gateway ? "Change agent" : "Set up concierge"}</Button>
            </span>
          </div>
        </SettingsCard>
        </div>

        <div ref={permissions}>
          <SettingsCard icon={<ShieldCheck size={16} />} title="Permissions" description={`What ${name}'s agents may ask yours for, and how often.`}>
            <NeighborPermissionsEditor neighbor={friend} name={name} permission={permission} pending={pending}
              onSave={(permission: NeighborPermission) => conciergeClient.setPermission(friendship, permission)}
              onSaveDailyLimit={dailyLimit => conciergeClient.setDailyLimit(friendship, dailyLimit)}
              onSaveFileDailyLimit={bytes => filesClient.setDailyLimit(friendship, bytes)}
              onLimit={onLimit} onEditWithGeneral={onInterview} />
          </SettingsCard>
        </div>

        <SettingsCard icon={<FolderOpen size={16} />} title="Files" description={`Files ${name} sends you are saved in their own folder on this machine.`}
          headerRight={<Button size="sm" onClick={() => { void filesClient.openFolder(friendship).catch((failure: unknown) => fail(failure, "Their folder could not be opened.")); }}>Open folder</Button>}>
          {quota && <p className="neighbor-note">Today you sent {formatBytes(quota.friend.used_bytes)} of the {formatBytes(quota.friend.limit_bytes)} {name} accepts, resets {formatUntil(quota.friend.resets_at, Date.now())}.</p>}
        </SettingsCard>

        <SettingsCard icon={<Bell size={16} />} title="Notifications" description={`Where ${name}'s messages reach you. Previews show only a first line.`}
          headerRight={<DropdownSelect aria-label="Notifications" value={notify} options={NOTIFY_CHOICES}
            onChange={value => {
              const next = value as NeighborNotify;
              const previous = notify;
              setNotify(next);
              void conciergeClient.setNotify(friendship, next).then(setNotify, (failure: unknown) => { setNotify(previous); fail(failure, "Notifications could not be changed."); });
            }} />} />

        <SettingsCard icon={<UserX size={16} />} title="Remove friend" className="neighbor-friend__danger"
          description={`${name}'s agents lose access to yours, and you would need a new request to reconnect.`}
          headerRight={!confirmRemove && <Button size="sm" variant="danger" disabled={pending} onClick={() => setConfirmRemove(true)}>Remove friend</Button>}>
          {confirmRemove && <div role="group" aria-label="Confirm removal" className="neighbor-confirm">
            <p>Remove {name}? Their concierge stops answering them.</p>
            <div className="neighbor-confirm__actions">
              <Button size="sm" variant="danger" disabled={pending} onClick={() => { void onRemove().then(removed => { if (removed) onBack(); }); }}>Confirm remove</Button>
              <Button size="sm" disabled={pending} onClick={() => setConfirmRemove(false)}>Keep {name}</Button>
            </div>
          </div>}
        </SettingsCard>
      </SettingsPage>
      </div>
    </section>
    {changing && <ChangeConciergeDialog friendship={friendship} name={name} currentProvider={gateway?.agent_type ?? null}
      client={conciergeClient} onClose={() => setChanging(false)} />}
  </div>;
}

/** One quiet line of today's numbers; a value we do not know yet is left out rather than guessed. */
function FriendStats({ friend, quota, history }: { friend: Neighbor; quota: NeighborFileQuota | null; history: HistoryEntry[] | null }) {
  const now = Date.now();
  const today = new Date(now).toDateString();
  const received = history?.filter(entry => entry.direction === "in" && new Date(entry.at).toDateString() === today).length;
  const last = history && history.length > 0 ? Math.max(...history.map(entry => entry.at)) : null;
  const chips = [
    typeof friend.sent_today === "number" ? (typeof friend.their_daily_limit === "number"
      ? `${friend.sent_today} of ${friend.their_daily_limit} sent today` : `${friend.sent_today} sent today`) : null,
    received !== undefined ? `${received} received today` : null,
    quota ? `${formatBytes(quota.friend.used_bytes)} of ${formatBytes(quota.friend.limit_bytes)} files today` : null,
    last !== null ? `last message ${formatAge(last, now)}` : null,
  ].filter((chip): chip is string => chip !== null);
  if (chips.length === 0) return null;
  return <p className="neighbor-friend__stats" aria-label="Today with this friend">{chips.map(chip => <Chip key={chip} variant="muted">{chip}</Chip>)}</p>;
}
