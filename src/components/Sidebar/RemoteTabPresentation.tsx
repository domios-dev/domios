import type { ReactNode } from "react";
import { cleanMachineName, machineLabel } from "../../lib/machineIdentity";
import { REMOTE_CHAT_COPY, remoteMachineName } from "../../lib/remoteChatState";
import { useRemoteChatStatus } from "../../lib/remoteChatStatus";
import { asleepDuration } from "../../lib/tabSleep";
import type { TabActivity } from "../../lib/tabActivity";
import { useMeshMachineIdentity, useMeshPeerName } from "../../lib/useMeshMachineIdentities";
import type { Tab } from "../../lib/types";
import type { GeneralAvailability } from "../../store/multiplexer/pairedWorkspaceProjection";
import { CursorTooltip } from "../shared/CursorTooltip";
import { MachineGlyph } from "../shared/MachineGlyph";
import { MachineProvenance } from "../shared/MachineProvenance";
import { namedManagerRole, useRemoteTabTeamName } from "./remoteOwnership";
import { remoteTabKindLabel } from "./remoteTabKindLabel";

export interface ObservedTabMachine {
  peerId: string;
  name: string;
  availability: GeneralAvailability;
  cached?: boolean;
}

export function useRemoteTabPresentation(tab: Tab, activity: TabActivity, observed?: ObservedTabMachine | null) {
  const peerId = observed?.peerId ?? tab.remote_peer_id;
  const remote = useRemoteChatStatus(tab.id, peerId);
  const peerName = useMeshPeerName(peerId);
  const identity = useMeshMachineIdentity(peerId, Boolean(peerId));
  const team = useRemoteTabTeamName(tab, peerId) ?? "No Team";
  if (!peerId) return null;
  const name = remoteMachineName(peerId, peerName, remote.peer?.name, observed?.name, tab.remote_peer_name);
  const availability = observed?.availability ?? remote.peer?.presence;
  const presence = remote.peer?.presence ?? availability;
  const online = presence === "online";
  const connection = online ? "online" : presence && presence !== "reconnecting" ? "offline" : "reconnecting";
  const reason = remote.state && REMOTE_CHAT_COPY[remote.state].label;
  const availabilityLabel = availability === "signed-out" ? "signed out"
    : availability === "update-required" ? "update required" : availability;
  const unavailable = reason || (availability === "online" ? null : availabilityLabel);
  const role = namedManagerRole(tab) ?? remoteTabKindLabel(tab);
  const duration = tab.frozen ? asleepDuration(tab, Date.now()) : null;
  const state = tab.frozen
    ? duration ? `asleep for ${duration}` : "asleep"
    : activity.launching ? "starting" : activity.status.toLowerCase();
  const label = `${role}, ${team}, on ${machineLabel(name, identity, [connection, unavailable === connection ? null : unavailable, state].filter(Boolean).join(", "))}`;
  return { peerId, name, identity, connection, online, state, unavailable, label };
}

type RemotePresentation = NonNullable<ReturnType<typeof useRemoteTabPresentation>>;

export function RemoteMachineSummary({ machine, stacked = false }: { machine: RemotePresentation; stacked?: boolean }) {
  return <span className={stacked ? "grid gap-1" : "flex min-w-0 items-center gap-1"}>
    <span className="flex min-w-0 items-center gap-1">
      <MachineGlyph {...machine.identity} />
      <span className="min-w-0 truncate">{machine.name}</span>
      <span aria-hidden="true">·</span>
      <span aria-hidden="true" className={`size-1.5 shrink-0 rounded-full ${machine.online ? "bg-[var(--ct-green)]" : "bg-[var(--ct-muted)]"}`} />
      <span className="shrink-0 whitespace-nowrap">{machine.connection}</span>
    </span>
    <span className="shrink-0 whitespace-nowrap">{stacked ? "" : "· "}{machine.state}</span>
  </span>;
}

export function RemoteTabIcon({ machine, children }: { machine: RemotePresentation; children: ReactNode }) {
  return <CursorTooltip as="span" active className="sidebar-remote-tab-icon" content={<RemoteMachineSummary machine={machine} stacked />}>
    {children}
    <span className="sidebar-remote-tab-icon__badge [&_svg]:size-2.5"><MachineGlyph {...machine.identity} /></span>
  </CursorTooltip>;
}

export function TeamRunMachine({ peerId, fallbackName }: { peerId?: string; fallbackName?: string | null }) {
  const resolved = useMeshPeerName(peerId);
  const name = peerId ? resolved ?? cleanMachineName(fallbackName, peerId) : undefined;
  return name ? <div className="flex min-w-0 items-baseline gap-1 px-2 pb-1 text-[length:var(--ct-font-size-meta)] text-[var(--ct-muted-fg)]">
    <span className="shrink-0">Runs on:</span>
    <MachineProvenance peerId={peerId} name={name} />
  </div> : null;
}
