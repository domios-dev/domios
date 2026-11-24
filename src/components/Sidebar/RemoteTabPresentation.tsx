import { useContext, type CSSProperties, type ReactNode } from "react";
import { deriveColor } from "../../lib/colors";
import { cleanMachineName, desaturateMachineColor, machineColor, machineGlyphColor, machineLabel, UNKNOWN_MACHINE, type MachineIdentity } from "../../lib/machineIdentity";
import { REMOTE_CHAT_COPY, remoteMachineName } from "../../lib/remoteChatState";
import { useRemoteChatStatus } from "../../lib/remoteChatStatus";
import { asleepDuration } from "../../lib/tabSleep";
import type { TabActivity } from "../../lib/tabActivity";
import { DEFAULT_THEME } from "../../lib/themeCss";
import { MachineAppearanceContext } from "../../lib/useMachineAppearance";
import { useMachineChromeAppearance } from "../../lib/useMachineChromeAppearance";
import { useMeshMachineIdentity, useMeshPeerName } from "../../lib/useMeshMachineIdentities";
import type { Tab } from "../../lib/types";
import type { GeneralAvailability } from "../../store/multiplexer/pairedWorkspaceProjection";
import { CursorTooltip } from "../shared/CursorTooltip";
import { MachineGlyph } from "../shared/MachineGlyph";
import { MachineStatus } from "../shared/MachineRow";
import { namedManagerRole, useRemoteTabTeamName } from "./remoteOwnership";
import { remoteTabKindLabel } from "./remoteTabKindLabel";

export interface ObservedTabMachine {
  peerId: string;
  name: string;
  availability: GeneralAvailability;
  cached?: boolean;
  teamName?: string;
}

export function useRemoteTabPresentation(tab: Tab, activity: TabActivity, observed?: ObservedTabMachine | null) {
  const peerId = observed?.peerId ?? tab.remote_peer_id;
  const remote = useRemoteChatStatus(tab.id, peerId);
  const peerName = useMeshPeerName(peerId);
  const identity = useMeshMachineIdentity(peerId, Boolean(peerId));
  const teamName = useRemoteTabTeamName(tab, peerId);
  const team = observed?.teamName ?? teamName ?? "No Team";
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
  return { peerId, name, identity, connection, state, unavailable, label };
}

type RemotePresentation = NonNullable<ReturnType<typeof useRemoteTabPresentation>>;

export function RemoteTabMachineGlyph({ machine }: { machine: RemotePresentation }) {
  const online = machine.connection === "online";
  const appearance = useMachineChromeAppearance(machine.peerId, online);
  const viewer = useContext(MachineAppearanceContext)?.appearance.theme ?? DEFAULT_THEME;
  const tone = appearance ? machineGlyphColor(
    desaturateMachineColor(appearance.theme.blue, online), deriveColor(viewer.background, 8), viewer.foreground,
  ) : "var(--ct-muted-fg)";
  return <MachineGlyph {...machine.identity} tone={tone} />;
}

export function RemoteMachineSummary({ machine, stacked = false }: { machine: RemotePresentation; stacked?: boolean }) {
  return <span className={stacked ? "grid gap-1" : "flex min-w-0 items-center gap-1"}>
    <span className="flex min-w-0 items-center gap-1">
      <RemoteTabMachineGlyph machine={machine} />
      <span className="min-w-0 truncate">{machine.name}</span>
      <span aria-hidden="true">·</span>
      <MachineStatus status={machine.connection} />
    </span>
    <span className="shrink-0 whitespace-nowrap">{stacked ? "" : "· "}{machine.state}</span>
  </span>;
}

export function RemoteTabIcon({ machine, children }: { machine: RemotePresentation; children: ReactNode }) {
  return <CursorTooltip as="span" active opaque className="sidebar-remote-tab-icon" content={<RemoteMachineSummary machine={machine} stacked />}>
    {children}
  </CursorTooltip>;
}

export function useTeamMachine(peerId?: string, fallbackName?: string | null) {
  const resolved = useMeshPeerName(peerId);
  const identity = useMeshMachineIdentity(peerId, Boolean(peerId));
  return peerId
    ? { name: resolved ?? cleanMachineName(fallbackName, peerId) ?? "Remote machine", identity: identity ?? UNKNOWN_MACHINE }
    : { name: "This machine" };
}

export function TeamMachinePill({ id, name, identity }: { id: string; name: string; identity?: MachineIdentity }) {
  return <span
    id={id}
    className="ct-neutral-badge sidebar-team-machine-pill"
    data-remote={identity ? "true" : undefined}
    title={name}
    style={identity ? { "--ct-plan-tone": `var(--ct-machine-${machineColor(identity.color)})` } as CSSProperties : undefined}
  >
    {identity && <MachineGlyph {...identity} />}
    <span className="sidebar-team-machine-pill__label min-w-0 truncate">{name}</span>
  </span>;
}
