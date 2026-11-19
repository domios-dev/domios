import { type ReactNode } from "react";
import { Laptop, RefreshCw, UserRound } from "lucide-react";
import { formatAge } from "../../lib/timeLabels";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { UI_BUTTON, UI_TEXT } from "../shared/design";
import { Button } from "../shared/Button";
import { BusyState } from "../shared/BusyState";
import { SettingsPill } from "../Settings/shared";
import { LockBadge } from "../billing/LockBadge";
import { lockedDimClass } from "../billing/freeLimitModel";
import { meshMachineLockedView } from "../billing/lockSurfaces";
import { plansAction } from "../billing/purchaseSwitch";
import { EMPTY_ACCOUNT_COPY, type AccountCanvasCard, type AccountMachineNode, type MachineCallout, type MachineTone, type PeerAccountFooter } from "./accountCanvas";
import { MACHINE_CARD_WIDTH } from "./webGraphLayout";
import type { MeshTarget } from "./accountMeshClient";
import type { MachineRowView } from "./accountMachines";
import { REAUTH_REQUIRED_COPY } from "./accountMachines";
import { MachineSessionControls } from "./MachineSessionControls";
import { MachineName } from "./MachineName";
import { MachineFooter, ACCESS_LABELS } from "./MachineFooter";
import type { PeerStatus } from "./useMeshData";
import { MachineGlyph } from "../shared/MachineGlyph";
import { machineLabel } from "../../lib/machineIdentity";
import { useMeshMachineIdentity } from "../../lib/useMeshMachineIdentities";
import { MachineJoinAction } from "./AddPeerPanel";

const PILL_TONE: Record<MachineTone, "green" | "accent" | "muted" | "amber" | "red"> = {
  ok: "green",
  progress: "accent",
  muted: "muted",
  warning: "amber",
  danger: "red",
};

const COMPACT_GHOST = `${UI_BUTTON.ghost} ${UI_BUTTON.compact}`;
const COMPACT_ACCENT = `${UI_BUTTON.accent} ${UI_BUTTON.compact}`;
const NODE_FRAME = "ct-card flex flex-col gap-2 px-3.5 py-3 text-left";

export function MachineReauthentication({ signingIn, onSignIn, onCancel }: { signingIn: boolean; onSignIn: () => void; onCancel?: () => void }) {
  return <div role="group" aria-label="Add this machine to your mesh" className="flex flex-col gap-2 px-3.5 py-2.5" onClick={(event) => event.stopPropagation()}>
    <p role="status" className="m-0 text-[12px] text-[var(--ct-muted-fg)]">{REAUTH_REQUIRED_COPY}</p>
    <Button size="compact" variant="primary" onClick={onSignIn} loading={signingIn}>{signingIn ? "Signing in…" : "Rejoin mesh"}</Button>
    {onCancel && <Button size="compact" onClick={onCancel}>Cancel</Button>}
  </div>;
}

export interface MachineNodeActions {
  onSignOut?: (row: MachineRowView) => Promise<boolean>;
  onRemove?: (row: MachineRowView, target: MeshTarget, alsoSignOut?: boolean) => Promise<boolean>;
  onRename?: (row: MachineRowView, name: string) => Promise<boolean>;
  onPlans: (key: string) => void;
  onDismiss: (key: string) => void;
}

function SessionControls({ row, busy, actions }: { row: MachineRowView; busy: boolean; actions: MachineNodeActions }) {
  const target = row.controls.revoke;
  if ((!row.canSignOut || !actions.onSignOut) && (!target || !actions.onRemove)) return null;
  return <MachineSessionControls name={row.name} busy={busy} onSignOut={row.canSignOut && actions.onSignOut ? () => actions.onSignOut!(row) : undefined}
    onRemove={target && actions.onRemove ? (alsoSignOut) => actions.onRemove!(row, target, alsoSignOut) : undefined} />;
}

function Callout({ callout, onRetry, onDismiss }: { callout: MachineCallout; onRetry?: () => void; onDismiss: () => void }) {
  if (callout.transient) return <p role="status" className={UI_TEXT.metaMuted}>{callout.message}</p>;
  return <InlineErrorNotice
    role="alert"
    description={{ title: callout.title, cause: callout.message, raw: callout.message, actions: [] }}
    actions={[
      ...(callout.retry && onRetry ? [{ label: "Try again", onClick: onRetry }] : []),
      { label: "Dismiss", ariaLabel: `Dismiss: ${callout.title}`, onClick: onDismiss },
    ]}
  />;
}

/** An account machine that is not drawn as a paired card: name, state and the one action that moves it forward. */
export function AccountMachineCard({ node, busy, now, actions, hint }: { node: AccountMachineNode; busy: boolean; now: number; actions: MachineNodeActions; hint?: ReactNode }) {
  const lock = meshMachineLockedView(node.row);
  const action = node.action;
  const seen = node.online || node.lastSeenAt === null ? null : formatAge(node.lastSeenAt * 1000, now);
  const identity = useMeshMachineIdentity(node.row.pairInstanceId, Boolean(node.row.pairInstanceId));
  return (
    <section aria-label={machineLabel(node.name, identity, node.online ? "online" : node.status)} data-status={node.row.status}
      className={NODE_FRAME}
      style={{ width: MACHINE_CARD_WIDTH }}>
      {node.callout && <Callout callout={node.callout} onDismiss={() => actions.onDismiss(node.key)} />}
      <div className="flex items-center gap-2 min-w-0">
        <MachineGlyph {...identity} size="tile" />
        <span className={`min-w-0 flex flex-1 text-[14px] font-medium text-[var(--ct-fg)] ${lockedDimClass(lock)}`}>
          <MachineName name={node.name} onRename={actions.onRename ? (name) => actions.onRename!(node.row, name) : undefined}
            disabledReason={!node.row.target || !node.row.pairInstanceId || !node.row.online ? "Connect to rename it, or rename it on that machine" : undefined} />
        </span>
        <LockBadge view={lock} />
        <SettingsPill tone={PILL_TONE[node.tone]}>{node.status}</SettingsPill>
      </div>
      <div role="status" aria-busy={node.row.connecting || undefined} className="text-[12px] text-[var(--ct-muted-fg)]">{node.row.connecting ? <BusyState inline label={node.detail} phase="work" /> : node.detail}</div>
      {seen && <div className={UI_TEXT.metaMuted}>Last seen {seen}</div>}
      {hint}
      <SessionControls row={node.row} busy={busy} actions={actions} />
      {action && (
        <div className="flex flex-wrap items-center gap-1.5 pt-0.5">
          {action?.kind === "plans" && (
            <button type="button" onClick={() => actions.onPlans(node.key)} aria-label={`See plans for ${node.name}`} className={COMPACT_ACCENT}>
              See plans
            </button>
          )}

        </div>
      )}
    </section>
  );
}

/** Account state and actions under a paired machine's card. */
export function PeerAccountFooterView({ footer, name, lastSeen, busy, actions, hint, peer }: {
  footer: PeerAccountFooter;
  name: string;
  lastSeen: string | null;
  busy: boolean;
  actions: MachineNodeActions;
  hint?: ReactNode;
  peer?: PeerStatus;
}) {
  const row = footer.row;
  const pending = peer?.pending;
  const label = pending ? "Waiting to pair" : row.connecting ? row.detail : ["endpoint_missing", "endpoint_stale", "waiting"].includes(row.status) ? "Unreachable" : footer.status;
  const secondary = pending ? "pairs when online" : row.status === "linked" ? ACCESS_LABELS[peer?.access_level ?? ""] : lastSeen ? `last seen ${lastSeen.replace(/(\d)([a-z]+)/g, "$1 $2")}` : null;
  const helper = !pending && ["endpoint_missing", "endpoint_stale", "expired", "signed_out", "blocked"].includes(row.status) ? row.detail : null;
  const target = row.controls.revoke;
  return <div onClick={(event) => event.stopPropagation()}>
    <MachineSessionControls name={name} busy={busy}
      identityPeer={actions.onRename && row.online && row.target ? row.pairInstanceId ?? undefined : undefined}
      onSignOut={row.canSignOut && actions.onSignOut ? () => actions.onSignOut!(row) : undefined}
      onRemove={target && actions.onRemove ? (alsoSignOut) => actions.onRemove!(row, target, alsoSignOut) : undefined}
      renderRow={(menu) => <MachineFooter label={label} secondary={secondary} tone={footer.tone} connecting={row.connecting}
        version={peer?.domios_version} build={peer?.domios_build} menu={menu}>
        {footer.plans && <button type="button" onClick={() => actions.onPlans(footer.key)} aria-label={`${plansAction("See plans")} for ${name}`} className={COMPACT_ACCENT}>{plansAction("See plans")}</button>}
      </MachineFooter>} />
    {helper && <p className="m-0 px-3.5 pb-2 text-[11px] text-[var(--ct-muted-fg)]">{helper}</p>}
    {(footer.callout || hint) && <div className="flex flex-col gap-2 px-3.5 pb-2.5">
      {footer.callout && <Callout callout={footer.callout} onDismiss={() => actions.onDismiss(footer.key)} />}
      {hint}
    </div>}
  </div>;
}

/** Stands in for the machines when there are none to draw: signed out, empty account, loading or unavailable. */
export function AccountStateCard({ card, signingIn, onSignIn, onCancel, onRetry }: {
  card: AccountCanvasCard;
  signingIn: boolean;
  onSignIn: () => void;
  onCancel?: () => void;
  onRetry: () => void;
}) {
  const frame = `${NODE_FRAME} ct-card--static`;
  if (card.kind === "loading") {
    return <section aria-label="My machines" aria-busy="true" className={frame} style={{ width: MACHINE_CARD_WIDTH }}>
      <BusyState inline label="Checking machine status" phase="work" className="text-[12px] text-[var(--ct-muted-fg)]" />
    </section>;
  }
  if (card.kind === "unavailable") {
    return <section aria-label="My machines" className={frame} style={{ width: MACHINE_CARD_WIDTH }}>
      <p role="status" className="m-0 text-[12px] text-[var(--ct-muted-fg)]">Your machines could not be loaded.</p>
      <div><button type="button" onClick={onRetry} className={COMPACT_GHOST}><RefreshCw size={12} aria-hidden="true" /> Try again</button></div>
    </section>;
  }
  if (card.kind === "sign_in") {
    return <section aria-label="Sign in to use Mesh" className={frame} style={{ width: MACHINE_CARD_WIDTH }}>
      <div className="flex items-center gap-2">
        <UserRound size={15} className="shrink-0 text-[var(--ct-accent)]" aria-hidden="true" />
        <span className="text-[14px] font-medium text-[var(--ct-fg)]">{card.blocked ? "Mesh is paused" : "Connect your machines"}</span>
      </div>
      <p role="status" className="m-0 text-[12px] text-[var(--ct-muted-fg)]">{card.headline}</p>
      <MachineJoinAction signingIn={signingIn} onSignIn={onSignIn} onCancel={onCancel} />
    </section>;
  }
  return <section aria-label="My machines" className={frame} style={{ width: MACHINE_CARD_WIDTH }}>
    <div className="flex items-center gap-2">
      <Laptop size={15} className="shrink-0 text-[var(--ct-muted-fg)]" aria-hidden="true" />
      <span className="text-[14px] font-medium text-[var(--ct-fg)]">{card.listFailed ? "Your machines could not be listed" : "No other machines yet"}</span>
    </div>
    <p role="status" className="m-0 text-[12px] text-[var(--ct-muted-fg)]">{card.listFailed ? "The account service did not answer. Your connections are unchanged." : EMPTY_ACCOUNT_COPY}</p>
    {card.listFailed && <div><button type="button" onClick={onRetry} className={COMPACT_GHOST}><RefreshCw size={12} aria-hidden="true" /> Try again</button></div>}
  </section>;
}

/** Account-wide notes that belong to no single machine, kept inside the canvas. */
export function AccountNotes({ notes }: { notes: string[] }) {
  if (notes.length === 0) return null;
  return (
    <div role="status" data-peer-card className="pointer-events-auto absolute bottom-3 left-3 z-10 flex max-w-[360px] flex-col gap-1 rounded-md border border-[var(--ct-border)] bg-[var(--ct-bg)] px-3 py-2 text-[12px] text-[var(--ct-muted-fg)] shadow-sm">
      {notes.map((note) => <p key={note} className="m-0">{note}</p>)}
    </div>
  );
}
