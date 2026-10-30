import { failureMessage, type MachineRowView, type MachineStatus, type MyMachinesView } from "./accountMachines";
import type { PeerStatus } from "./useMeshData";
import type { MachineFailure } from "./useMyMachines";

export const EMPTY_ACCOUNT_COPY = "Sign in to Domios with this account on another machine. It connects automatically.";

/** The single account node the canvas shows when there are no machines to draw. */
export type AccountCanvasCard =
  | { kind: "loading" }
  | { kind: "unavailable" }
  | { kind: "sign_in"; headline: string; blocked: boolean }
  | { kind: "empty"; listFailed: boolean };

export type MachineAction = { kind: "plans" } | null;

export type MachineTone = "ok" | "progress" | "muted" | "warning" | "danger";

/** One account machine drawn as its own node because no paired peer card represents it. */
export interface AccountMachineNode {
  key: string;
  row: MachineRowView;
  name: string;
  status: string;
  tone: MachineTone;
  detail: string;
  online: boolean | null;
  lastSeenAt: number | null;
  locked: boolean;
  action: MachineAction;
  callout: MachineCallout | null;
}

export interface MachineCallout {
  title: string;
  message: string;
  retry: boolean;
  transient?: boolean;
}

/** The account facts a paired peer card carries in its footer. */
export interface PeerAccountFooter {
  key: string;
  row: MachineRowView;
  status: string;
  tone: MachineTone;
  locked: boolean;
  plans: boolean;
  callout: MachineCallout | null;
}

export interface AccountCanvas {
  card: AccountCanvasCard | null;
  nodes: AccountMachineNode[];
  peerFooters: ReadonlyMap<string, PeerAccountFooter>;
  notes: string[];
}

const STATUS: Record<MachineStatus, { label: string; tone: MachineTone }> = {
  signed_out: { label: "Signed out", tone: "muted" },
  linked: { label: "Connected", tone: "ok" },
  linking: { label: "Connecting", tone: "progress" },
  endpoint_missing: { label: "Unavailable", tone: "muted" },
  endpoint_stale: { label: "Stale", tone: "warning" },
  expired: { label: "Expired", tone: "warning" },
  revoked: { label: "Removed", tone: "danger" },
  over_limit: { label: "Locked", tone: "warning" },
  unverified: { label: "Unverified", tone: "warning" },
  blocked: { label: "Blocked", tone: "danger" },
  other_account: { label: "Other account", tone: "danger" },
};

export interface AccountCanvasInput {
  view: MyMachinesView;
  peers: readonly PeerStatus[];
  loadFailed: boolean;
  listFailed: boolean;
  failures: ReadonlyMap<string, MachineFailure>;
}

export function accountCanvas({ view, peers, loadFailed, listFailed, failures }: AccountCanvasInput): AccountCanvas {
  const empty = { nodes: [], peerFooters: new Map<string, PeerAccountFooter>(), notes: [] };
  if (view.kind === "loading") return { ...empty, card: { kind: loadFailed ? "unavailable" : "loading" } };
  const rows = view.kind === "account" ? view.machines : [];
  const peerIds = new Set(peers.map((peer) => peer.instance_id));
  const nodes: AccountMachineNode[] = [];
  const peerFooters = new Map<string, PeerAccountFooter>();
  for (const row of rows) {
    const callout = row.status === "signed_out" ? null : machineCallout(failures.get(row.key));
    const { label, tone } = callout?.transient ? { label: "Busy", tone: "muted" as const } : STATUS[row.status];
    const locked = row.status === "over_limit";
    if (row.pairInstanceId && peerIds.has(row.pairInstanceId)) {
      peerFooters.set(row.pairInstanceId, { key: row.key, row, status: label, tone, locked, plans: locked || !!row.controls.plans, callout });
      continue;
    }
    if (row.status === "revoked" && !row.pairInstanceId) continue;
    nodes.push({
      key: row.key,
      row,
      name: row.name,
      status: label,
      tone,
      detail: callout?.transient ? callout.message : row.detail,
      online: row.online,
      lastSeenAt: row.lastSeenAt,
      locked,
      action: machineAction(row),
      callout,
    });
  }
  const card: AccountCanvasCard | null = view.kind === "sign_in"
    ? { kind: "sign_in", headline: view.headline, blocked: view.blocked }
    : !view.removed && !view.reauthRequired && !view.machineLimit && nodes.length === 0 && peerFooters.size === 0 && peers.length === 0
      ? { kind: "empty", listFailed }
      : null;
  return { card, nodes, peerFooters, notes: view.notes };
}

function machineAction(row: MachineRowView): MachineAction {
  return row.status === "over_limit" || row.controls.plans ? { kind: "plans" } : null;
}

function machineCallout(entry: MachineFailure | undefined): MachineCallout | null {
  if (!entry) return null;
  const transient = entry.failure.reason === "rate-limited";
  const retry = false;
  return { title: transient ? "Mesh is busy" : `Could not ${entry.operation}`, message: failureMessage(entry.failure, undefined, entry.retryAt), retry, ...(transient ? { transient } : {}) };
}

/** Accepts epoch seconds, epoch milliseconds or an ISO timestamp. */
export function lastSeenMs(value: number | string | null | undefined): number | null {
  if (value == null || value === "") return null;
  const numeric = typeof value === "number" ? value : Number(value);
  if (Number.isFinite(numeric)) return numeric < 1e12 ? numeric * 1000 : numeric;
  const parsed = Date.parse(String(value));
  return Number.isFinite(parsed) ? parsed : null;
}
