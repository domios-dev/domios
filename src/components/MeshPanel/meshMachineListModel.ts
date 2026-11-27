import { appVersion, buildHash } from "../../lib/buildStamp";
import { cloudMachineCardView, cloudSizeOptions } from "../../lib/cloudMachines/stateModel";
import type { CloudLifecycleState, CloudReceipt, CloudUsagePayload } from "../../lib/cloudMachines/types";
import { RELAY_PROTOCOL_VERSION } from "../../lib/types";
import { webLinkWithTokenAndRelay } from "../../lib/webAccess";
import {
  lastSeenMs,
  type AccountCanvas,
  type AccountMachineNode,
  type MachineCallout,
  type MachineTone,
} from "./accountCanvas";
import type { MachineRowView, MachineStatus } from "./accountMachines";
import type { MeshTarget } from "./accountMeshClient";
import { ACCESS_LABELS } from "./MachineFooter";
import type { CanvasAccount } from "./MeshCanvas";
import type { MeshCollapsedGroupOverrides } from "./meshViewModeModel";
import { findMachineStorageEntry, machineStorageTier, storageTierLabel, type CloudStorageTier } from "../Usage/cloudStorageModel";
import { privateNetworkLabel, type TailscaleState } from "./tailscaleSetup";
import type { PeerStatus } from "./useMeshData";

export type MeshMachineGroupId =
  | "this_machine"
  | "online"
  | "cloud"
  | "needs_attention"
  | "offline";

export const MESH_MACHINE_GROUP_ORDER: readonly MeshMachineGroupId[] = [
  "this_machine",
  "online",
  "cloud",
  "needs_attention",
  "offline",
];

export const MESH_MACHINE_GROUP_LABELS: Record<MeshMachineGroupId, string> = {
  this_machine: "This machine",
  online: "Online",
  cloud: "Cloud",
  needs_attention: "Needs attention",
  offline: "Offline",
};

export const OFFLINE_AUTO_COLLAPSE_THRESHOLD = 3;
export const FILTER_VISIBLE_THRESHOLD = 20;

const NEEDS_ATTENTION_STATUSES: ReadonlySet<MachineStatus> = new Set([
  "auth_refused",
  "membership_expired",
  "peer_needs_update",
  "expired",
  "signed_out",
  "over_limit",
  "unverified",
  "blocked",
  "other_account",
]);

const TONE_SEVERITY: Record<MachineTone, number> = {
  danger: 0,
  warning: 1,
  progress: 2,
  muted: 3,
  ok: 4,
};

export type MeshMachineRowKind = "self" | "peer" | "cloud" | "account";

export interface MeshMachineRow {
  id: string;
  group: MeshMachineGroupId;
  kind: MeshMachineRowKind;
  name: string;
  displayName?: string | null;
  sameNameOthers?: number;
  ip: string;
  isThisMachine: boolean;
  isCloud: boolean;
  cloudSizeMeta?: string | null;
  cloudSizeSpecs?: string | null;
  cloudState?: CloudLifecycleState;
  storageTier?: CloudStorageTier;
  statusLabel: string;
  detailLine?: string | null;
  tone: MachineTone;
  connecting: boolean;
  statusIcon?: "moon" | null;
  online: boolean;
  version?: string | null;
  build?: string | null;
  lastSeenMs: number | null;
  showLastSeen: boolean;
  peerId?: string | null;
  canOpen: boolean;
  locked: boolean;
  plansAction: boolean;
  signedOutRow: boolean;
  canRename: boolean;
  renameUnavailable: boolean;
  canSignOut: boolean;
  revokeTarget: MeshTarget | null;
  cloudMachine?: CloudReceipt;
  cloudUsage?: CloudUsagePayload | null;
  peer?: PeerStatus;
  accountRow?: MachineRowView;
  accountNode?: AccountMachineNode;
  callout?: MachineCallout | null;
  accountKey?: string;
  needsReauth?: boolean;
  removed?: boolean;
  meshOff?: boolean;
  reachabilityHint?: string;
  webUrl?: string | null;
}

export interface MeshMachineGroup {
  id: MeshMachineGroupId;
  label: string;
  rows: MeshMachineRow[];
}

export interface MeshMachineListInput {
  thisMachine: { name: string; ip: string };
  peers: readonly PeerStatus[];
  account?: Pick<
    CanvasAccount,
    | "selfName"
    | "selfDisplayName"
    | "selfSameNameOthers"
    | "canvas"
    | "reauthRequired"
    | "removed"
    | "onRenameSelf"
    | "actions"
  >;
  cloudMachines?: readonly CloudReceipt[];
  cloudUsage?: CloudUsagePayload | null;
  meshEnabled?: boolean;
  tailscaleState?: TailscaleState;
  localVersion?: string;
  localBuild?: string;
  webUiBaseUrl?: string | null;
  webUiToken?: string | null;
}

export function classifyAccountStatus(
  status: MachineStatus,
  online: boolean | null,
  pending = false,
): Exclude<MeshMachineGroupId, "this_machine" | "cloud"> {
  if (NEEDS_ATTENTION_STATUSES.has(status)) return "needs_attention";
  if (status === "linking") return "online";
  if (!pending && (status === "linked" || online === true)) return "online";
  return "offline";
}

export function deduplicateMeshInputs(
  peers: readonly PeerStatus[],
  accountCanvas?: AccountCanvas,
  cloudMachines?: readonly CloudReceipt[],
) {
  const activeCloud = (cloudMachines ?? []).filter((machine) => machine.state !== "deleted");
  const cloudPeerIds = new Set<string>();
  for (const machine of activeCloud) {
    if (machine.peer_instance_id) cloudPeerIds.add(machine.peer_instance_id);
    if (machine.installation_id) cloudPeerIds.add(machine.installation_id);
    cloudPeerIds.add(machine.machine_id);
  }
  const nonCloudPeers = peers.filter((peer) => !cloudPeerIds.has(peer.instance_id));
  const accountNodes = (accountCanvas?.nodes ?? []).filter(
    (node) =>
      !cloudPeerIds.has(node.key) &&
      (!node.row.pairInstanceId || !cloudPeerIds.has(node.row.pairInstanceId)),
  );
  return { activeCloud, cloudPeerIds, nonCloudPeers, accountNodes };
}

export function countProjectedMeshMachines(
  peers: readonly PeerStatus[],
  accountCanvas?: AccountCanvas,
  cloudMachines?: readonly CloudReceipt[],
): number {
  const { activeCloud, nonCloudPeers, accountNodes } = deduplicateMeshInputs(
    peers,
    accountCanvas,
    cloudMachines,
  );
  return 1 + nonCloudPeers.length + activeCloud.length + accountNodes.length;
}

export function projectMeshMachineRows({
  thisMachine,
  peers,
  account,
  cloudMachines,
  cloudUsage,
  meshEnabled = true,
  tailscaleState,
  localVersion,
  localBuild,
  webUiBaseUrl = null,
  webUiToken = null,
}: MeshMachineListInput): MeshMachineRow[] {
  const { activeCloud, nonCloudPeers, accountNodes } = deduplicateMeshInputs(
    peers,
    account?.canvas,
    cloudMachines,
  );
  const rows: MeshMachineRow[] = [];

  const networkLabel = privateNetworkLabel(tailscaleState);
  const selfStatusLabel = !meshEnabled
    ? "Mesh off"
    : account?.removed
      ? "Removed"
      : account?.reauthRequired
        ? "Sign-in needed"
        : "This machine";
  const selfTone: MachineTone = !meshEnabled
    ? "muted"
    : account?.removed
      ? "danger"
      : account?.reauthRequired
        ? "warning"
        : "ok";

  rows.push({
    id: "__self__",
    group: "this_machine",
    kind: "self",
    name: account?.selfName ?? thisMachine.name,
    displayName: account?.selfDisplayName,
    sameNameOthers: account?.selfSameNameOthers,
    ip: thisMachine.ip,
    isThisMachine: true,
    isCloud: false,
    statusLabel: selfStatusLabel,
    detailLine: networkLabel ?? (thisMachine.ip || null),
    tone: selfTone,
    connecting: false,
    statusIcon: null,
    online: meshEnabled,
    version: localVersion ?? appVersion(),
    build: localBuild ?? buildHash(),
    lastSeenMs: null,
    showLastSeen: false,
    peerId: null,
    canOpen: true,
    locked: false,
    plansAction: false,
    signedOutRow: false,
    canRename: Boolean(account?.onRenameSelf),
    renameUnavailable: false,
    canSignOut: false,
    revokeTarget: null,
    needsReauth: Boolean(account?.reauthRequired && !account?.removed),
    removed: Boolean(account?.removed),
    meshOff: !meshEnabled,
    webUrl: webUiBaseUrl ? webLinkWithTokenAndRelay(webUiBaseUrl, webUiToken, null) : null,
  });

  for (const peer of nonCloudPeers) {
    const footer = account?.canvas.peerFooters.get(peer.instance_id);
    const row = footer?.row;
    const signedOut = row?.status === "signed_out";
    const group = row
      ? classifyAccountStatus(row.status, peer.online && !signedOut, Boolean(peer.pending))
      : !peer.pending && peer.online
        ? "online"
        : "offline";
    const statusLabel = peer.pending
      ? "Waiting to pair"
      : row?.connecting
        ? "Connecting"
        : row && ["endpoint_missing", "endpoint_stale", "waiting"].includes(row.status)
          ? "Offline"
          : footer?.status ?? (peer.online ? "Connected" : "Offline");
    const detailLine = peer.pending
      ? "Pairs when online"
      : row?.connecting
        ? row.detail
        : row?.status === "linked" || (!row && peer.online)
          ? ACCESS_LABELS[peer.access_level] ?? null
          : peer.reachabilityHint ?? row?.detail ?? (peer.online ? null : "Unreachable");
    const seen = lastSeenMs(peer.last_seen) ?? lastSeenMs(row?.lastSeenAt);
    const online = !signedOut && !peer.pending && peer.online;
    const canRename = Boolean(row?.target && row.online && peer.online && account?.actions.onRename);
    const renameUnavailable = Boolean(row && (!row.target || !row.online || !peer.online));

    rows.push({
      id: peer.instance_id,
      group,
      kind: "peer",
      name: row?.name ?? peer.name,
      displayName: row?.displayName,
      sameNameOthers: row?.sameNameOthers,
      ip: peer.ip,
      isThisMachine: false,
      isCloud: false,
      statusLabel,
      detailLine,
      tone: footer?.tone ?? (peer.online ? "ok" : "muted"),
      connecting: Boolean(row?.connecting),
      online,
      version: peer.domios_version,
      build: peer.domios_build,
      lastSeenMs: seen,
      showLastSeen: !online && seen !== null,
      peerId: peer.instance_id,
      canOpen: !signedOut && online,
      locked: Boolean(footer?.locked),
      plansAction: Boolean(footer?.plans),
      signedOutRow: signedOut,
      canRename,
      renameUnavailable,
      canSignOut: Boolean(row?.canSignOut && account?.actions.onSignOut),
      revokeTarget: row?.controls.revoke ?? null,
      peer,
      accountRow: row,
      callout: footer?.callout ?? null,
      accountKey: footer?.key,
      reachabilityHint: peer.reachabilityHint,
      webUrl:
        online && peer.relay_protocol_version === RELAY_PROTOCOL_VERSION && webUiBaseUrl
          ? webLinkWithTokenAndRelay(webUiBaseUrl, webUiToken, { id: peer.instance_id })
          : null,
    });
  }

  const sizeCatalog = cloudSizeOptions(cloudUsage);
  for (const machine of activeCloud) {
    const matchedPeer = peers.find(
      (candidate) =>
        candidate.instance_id === machine.peer_instance_id ||
        candidate.instance_id === machine.installation_id ||
        candidate.instance_id === machine.machine_id,
    );
    const footer = matchedPeer ? account?.canvas.peerFooters.get(matchedPeer.instance_id) : undefined;
    const matchedNode = account?.canvas.nodes.find(
      (node) =>
        node.key === machine.installation_id ||
        node.key === machine.machine_id ||
        (node.row.pairInstanceId && node.row.pairInstanceId === machine.peer_instance_id),
    );
    const row = footer?.row ?? matchedNode?.row;
    const view = cloudMachineCardView(machine, cloudUsage);
    const sizeOption = sizeCatalog.find((entry) => entry.id === machine.size);
    const preparing = view.state === "creating" || view.state === "enrolling";
    const machineStorage = findMachineStorageEntry(cloudUsage?.storage?.machines, machine);
    // Cloud rows must use the cloud machine's own activity/creation timestamps, never the owner's account auth_time.
    const seenCandidates = [
      lastSeenMs(matchedPeer?.last_seen),
      lastSeenMs(machine.last_seen_at_ms),
      lastSeenMs(machineStorage?.stopped_at),
      lastSeenMs(machine.created_at_ms),
      lastSeenMs(machineStorage?.created_at_ms),
      lastSeenMs(row?.admittedAt),
    ].filter((value): value is number => value !== null);
    const seen = seenCandidates.length > 0 ? Math.max(...seenCandidates) : null;
    const canRename = Boolean(row?.target && matchedPeer?.online && account?.actions.onRename);
    const renameUnavailable = !row || !row.target || !matchedPeer?.online;
    const storageTier = machineStorageTier(machine.storage_tier, machineStorage?.tier);
    const archiveLabel = storageTier === "evicting"
      ? storageTierLabel(storageTier)
      : storageTier === "cold" ? "Archived" : null;
    const statusLabel = archiveLabel ?? (view.state === "waking" ? "Waking up" : view.footerLabel);
    const tone: MachineTone = view.state === "out_of_credits" ? "warning" : view.footerTone;

    rows.push({
      id: `cloud:${machine.machine_id}`,
      group: "cloud",
      kind: "cloud",
      name: machine.name,
      displayName: row?.displayName,
      sameNameOthers: row?.sameNameOthers,
      ip: matchedPeer?.ip ?? "",
      isThisMachine: false,
      isCloud: true,
      cloudSizeMeta: sizeOption?.specsLabel ? `${view.sizeLabel} · ${sizeOption.specsLabel}` : view.sizeLabel,
      cloudSizeSpecs: sizeOption?.specsLabel ?? view.sizeLabel,
      cloudState: view.state,
      storageTier,
      statusLabel,
      detailLine: sizeOption?.specsLabel ? `${view.sizeLabel} · ${sizeOption.specsLabel}` : view.sizeLabel,
      tone,
      connecting: view.connecting,
      statusIcon: view.statusIcon,
      online: view.listOnline,
      version: view.showVersion ? matchedPeer?.domios_version : null,
      build: view.showVersion ? matchedPeer?.domios_build : null,
      lastSeenMs: seen,
      showLastSeen: !view.listOnline && seen !== null,
      peerId: matchedPeer?.instance_id ?? null,
      canOpen: !preparing && view.state !== "failed",
      locked: false,
      plansAction: false,
      signedOutRow: false,
      canRename,
      renameUnavailable,
      canSignOut: false,
      revokeTarget: null,
      cloudMachine: machine,
      cloudUsage,
      peer: matchedPeer,
      accountRow: row,
      reachabilityHint: matchedPeer?.reachabilityHint,
    });
  }

  for (const node of accountNodes) {
    const seen = lastSeenMs(node.lastSeenAt);
    const online = node.online === true;
    const canRename = Boolean(node.row.target && node.row.pairInstanceId && node.row.online && account?.actions.onRename);
    const renameUnavailable = !node.row.target || !node.row.pairInstanceId || !node.row.online;
    const group = classifyAccountStatus(node.row.status, node.online, false);
    const statusLabel =
      group === "offline" && node.row.status !== "revoked"
        ? "Offline"
        : node.status;

    rows.push({
      id: `account:${node.key}`,
      group,
      kind: "account",
      name: node.name,
      displayName: node.row.displayName,
      sameNameOthers: node.row.sameNameOthers,
      ip: "",
      isThisMachine: false,
      isCloud: false,
      statusLabel,
      detailLine: node.detail,
      tone: group === "offline" && node.row.status !== "revoked" ? "muted" : node.tone,
      connecting: Boolean(node.row.connecting),
      online,
      version: null,
      build: null,
      lastSeenMs: seen,
      showLastSeen: !online && seen !== null,
      peerId: node.row.pairInstanceId,
      canOpen: false,
      locked: node.locked,
      plansAction: node.action?.kind === "plans",
      signedOutRow: node.row.status === "signed_out",
      canRename,
      renameUnavailable,
      canSignOut: Boolean(node.row.canSignOut && account?.actions.onSignOut),
      revokeTarget: node.row.controls.revoke,
      accountRow: node.row,
      accountNode: node,
      callout: node.callout,
      accountKey: node.key,
    });
  }

  return rows;
}

function compareByName(left: MeshMachineRow, right: MeshMachineRow): number {
  const byName = left.name.localeCompare(right.name, undefined, { sensitivity: "base" });
  if (byName !== 0) return byName;
  return left.id.localeCompare(right.id);
}

export function compareRowsByLastSeen(left: MeshMachineRow, right: MeshMachineRow): number {
  const leftSeen = left.lastSeenMs;
  const rightSeen = right.lastSeenMs;
  if (leftSeen !== null && rightSeen !== null && leftSeen !== rightSeen) {
    return rightSeen - leftSeen;
  }
  if (leftSeen !== null && rightSeen === null) return -1;
  if (leftSeen === null && rightSeen !== null) return 1;
  return compareByName(left, right);
}

function compareNeedsAttention(left: MeshMachineRow, right: MeshMachineRow): number {
  const severityDiff = TONE_SEVERITY[left.tone] - TONE_SEVERITY[right.tone];
  if (severityDiff !== 0) return severityDiff;
  return compareRowsByLastSeen(left, right);
}

export function groupMeshMachineRows(rows: readonly MeshMachineRow[]): MeshMachineGroup[] {
  const buckets = new Map<MeshMachineGroupId, MeshMachineRow[]>();
  for (const row of rows) {
    const list = buckets.get(row.group);
    if (list) list.push(row);
    else buckets.set(row.group, [row]);
  }
  const groups: MeshMachineGroup[] = [];
  for (const id of MESH_MACHINE_GROUP_ORDER) {
    const items = buckets.get(id);
    if (!items || items.length === 0) continue;
    if (id === "online" || id === "cloud") {
      items.sort(compareByName);
    } else if (id === "needs_attention") {
      items.sort(compareNeedsAttention);
    } else {
      items.sort(compareRowsByLastSeen);
    }
    groups.push({
      id,
      label: MESH_MACHINE_GROUP_LABELS[id],
      rows: items,
    });
  }
  return groups;
}

export function filterMeshMachineRows(rows: readonly MeshMachineRow[], query: string): MeshMachineRow[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return [...rows];
  return rows.filter((row) =>
    row.name.toLowerCase().includes(needle) ||
    row.statusLabel.toLowerCase().includes(needle) ||
    (row.detailLine?.toLowerCase().includes(needle) ?? false) ||
    (row.version?.toLowerCase().includes(needle) ?? false),
  );
}

export function isGroupCollapsed(
  groupId: MeshMachineGroupId,
  rowCount: number,
  overrides: MeshCollapsedGroupOverrides,
): boolean {
  if (typeof overrides[groupId] === "boolean") return overrides[groupId]!;
  return groupId === "offline" && rowCount > OFFLINE_AUTO_COLLAPSE_THRESHOLD;
}

export function toggleCollapsedGroup(
  overrides: MeshCollapsedGroupOverrides,
  groupId: MeshMachineGroupId,
  rowCount: number,
): MeshCollapsedGroupOverrides {
  const currentlyCollapsed = isGroupCollapsed(groupId, rowCount, overrides);
  return { ...overrides, [groupId]: !currentlyCollapsed };
}

export function collapsedGroupSummary(rows: readonly MeshMachineRow[], maxNames: number): string {
  const shown = rows.slice(0, maxNames).map((row) => row.name);
  const remaining = rows.length - shown.length;
  if (remaining <= 0) return shown.join(", ");
  return `${shown.join(", ")} and ${remaining} more`;
}
