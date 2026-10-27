import { useEffect, useRef, useState } from "react";
import { Bell, Bot, FolderOpen, MessagesSquare, ShieldCheck, UserX } from "lucide-react";
import { AccountAvatar } from "../Titlebar/AccountAvatar";
import { accountInitials } from "../Settings/accountModel";
import { SettingsCard, SettingsPage, SettingsPill } from "../Settings/shared";
import { BackPill } from "../shared/BackPill";
import { Button } from "../shared/Button";
import { DropdownMenu, DropdownSelect } from "../shared/DropdownMenu";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { RowMenuButton } from "../shared/RowMenuButton";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { useMuxSelector } from "../../store/multiplexer";
import { ChangeConciergeDialog } from "./ChangeConciergeDialog";
import { focusTab, workspaceOfTab } from "./neighborAgent";
import type { NeighborConciergeClient, NeighborNotify } from "./neighborConciergeClient";
import { openNeighbor } from "./neighborFocus";
import { neighborName, type Neighbor } from "./neighborModel";
import { MissingConciergeNotice } from "./NeighborPanel";
import { NeighborPermissionsEditor } from "./NeighborPermissionsEditor";
import { formatBytes, neighborFilesClient, type NeighborFileQuota, type NeighborFilesClient } from "./neighborFilesClient";
import { formatUntil } from "../../lib/timeLabels";
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
export function NeighborFriendPage({ friend, locked, enabled, pending, focusPermissions, runtimeClient, conciergeClient, filesClient = neighborFilesClient,
  onBack, onTalkAbout, onInterview, onLimit, onRemove, onPlans }: {
  friend: Neighbor; locked: boolean; enabled: boolean; pending: boolean; focusPermissions: boolean;
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
  useEffect(() => { if (focusPermissions) permissions.current?.scrollIntoView({ block: "start" }); }, [focusPermissions]);

  const fail = (failure: unknown, fallback: string) => setError(failure instanceof Error ? failure.message : fallback);
  const status = !enabled ? "Neighbors is off" : locked ? "paused by plan" : "connected";

  return <div className="neighbor-friend absolute inset-0 z-10 flex flex-col bg-[var(--ct-bg)] pt-[var(--ct-safe-top,0px)]">
    <div className="ct-jobs-header ct-settings-head">
      <BackPill label="Neighborhood" onBack={onBack} />
      <h2 className="ct-settings-head__title">{name}</h2>
      <span className="ml-auto" />
      <DropdownMenu menuLabel={`${name} actions`} items={[
        { label: `Talk about ${name}`, onClick: onTalkAbout },
        { label: "Edit with General", onClick: onInterview },
      ]} trigger={<RowMenuButton label={`More actions for ${name}`} title="More actions" />} />
    </div>
    <div className="flex-1 overflow-y-auto px-5 py-5">
      <SettingsPage title={name}>
        <header className="neighbor-friend__hero">
          <span className="neighbor-friend__avatar"><AccountAvatar url={friend.photoURL} initial={accountInitials(name)} label="" size="node" /></span>
          <div className="min-w-0">
            <p className="neighbor-friend__name">{name}</p>
            <p className="neighbor-status"><span className="neighbor-dot" data-tone={enabled && !locked ? "ok" : "muted"} aria-hidden="true" />{status}
              <span className="neighbor-note">, since {new Date(friend.since * 1000).toLocaleDateString()}</span></p>
            {receiver && <p className="neighbor-note">{receivingLine(receiver)}</p>}
          </div>
        </header>
        {error && <InlineErrorNotice error={error} operation={`managing ${name}`} role="alert" />}
        {concierge?.concierge_missing && !gateway && <MissingConciergeNotice status={concierge} onSetUpWithGeneral={onInterview}
          onEditPermissions={() => permissions.current?.scrollIntoView({ block: "start", behavior: "smooth" })} />}

        <SettingsCard icon={<MessagesSquare size={16} />} title="Conversation"
          description={locked ? "Messages are paused by your plan. You can keep and manage this connection." : `What ${name} asked and what you and your agents answered.`}
          headerRight={locked ? <Button size="sm" variant="accent" onClick={onPlans}>View plans</Button>
            : <Button size="sm" variant="accent" onClick={() => openNeighbor(friendship)}>Open conversation</Button>} />

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
    {changing && <ChangeConciergeDialog friendship={friendship} name={name} currentProvider={gateway?.agent_type ?? null}
      client={conciergeClient} onClose={() => setChanging(false)} />}
  </div>;
}
