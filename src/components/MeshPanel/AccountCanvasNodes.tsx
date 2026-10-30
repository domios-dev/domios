import { useId, useState, type ReactNode } from "react";
import { Laptop, RefreshCw, UserRound } from "lucide-react";
import { formatAge } from "../../lib/timeLabels";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { UI_BUTTON, UI_TEXT } from "../shared/design";
import { BusyState } from "../shared/BusyState";
import { SettingsPill } from "../Settings/shared";
import { LockBadge } from "../billing/LockBadge";
import { lockedDimClass } from "../billing/freeLimitModel";
import { meshMachineLockedView } from "../billing/lockSurfaces";
import { EMPTY_ACCOUNT_COPY, type AccountCanvasCard, type AccountMachineNode, type MachineCallout, type MachineTone, type PeerAccountFooter } from "./accountCanvas";
import { MACHINE_CARD_WIDTH } from "./webGraphLayout";
import type { MeshTarget } from "./accountMeshClient";
import type { MachineRowView } from "./accountMachines";
import { REAUTH_REQUIRED_COPY } from "./accountMachines";
import { MachineSessionControls } from "./MachineSessionControls";
import { MachineName } from "./MachineName";

const PILL_TONE: Record<MachineTone, "green" | "accent" | "muted" | "amber" | "red"> = {
  ok: "green",
  progress: "accent",
  muted: "muted",
  warning: "amber",
  danger: "red",
};

const COMPACT_GHOST = `${UI_BUTTON.ghost} ${UI_BUTTON.compact}`;
const COMPACT_ACCENT = `${UI_BUTTON.accent} ${UI_BUTTON.compact}`;
const NODE_FRAME = "flex flex-col gap-2 rounded-md bg-[var(--ct-bg)] px-3.5 py-3 text-left shadow-sm";

export function MachineReauthentication({ signingIn, onSignIn }: { signingIn: boolean; onSignIn: () => void }) {
  return <div role="group" aria-label="Add this machine to your mesh" className="flex flex-col gap-2 border-t border-[var(--ct-border)] px-3.5 py-2.5" onClick={(event) => event.stopPropagation()}>
    <p role="status" className="m-0 text-[12px] text-[var(--ct-muted-fg)]">{REAUTH_REQUIRED_COPY}</p>
    <button type="button" onClick={onSignIn} disabled={signingIn} aria-busy={signingIn} className={`${UI_BUTTON.primary} ${UI_BUTTON.compact}`}>
      {signingIn ? "Signing in…" : "Rejoin mesh"}
    </button>
  </div>;
}

export interface MachineNodeActions {
  onSignOut?: (row: MachineRowView) => Promise<boolean>;
  onRemove?: (row: MachineRowView, target: MeshTarget, alsoSignOut?: boolean) => Promise<boolean>;
  onDecide?: (row: MachineRowView, notificationId: string, approve: boolean) => void;
  onRename?: (row: MachineRowView, name: string) => Promise<boolean>;
  onConnect: (row: MachineRowView, target: MeshTarget) => void;
  onDisconnect: (row: MachineRowView, instanceId: string) => void;
  onPlans: (key: string) => void;
  onDismiss: (key: string) => void;
}

function SessionControls({ row, busy, actions }: { row: MachineRowView; busy: boolean; actions: MachineNodeActions }) {
  const target = row.controls.revoke;
  if (!row.canSignOut || !actions.onSignOut) return null;
  return <MachineSessionControls name={row.name} busy={busy} onSignOut={() => actions.onSignOut!(row)}
    onRemove={target && actions.onRemove ? (alsoSignOut) => actions.onRemove!(row, target, alsoSignOut) : undefined} />;
}

function ApprovalControls({ row, busy, actions }: { row: MachineRowView; busy: boolean; actions: MachineNodeActions }) {
  const request = row.linkRequest;
  if (row.status !== "approval_requested" || !request?.notification_id) return null;
  const id = request.notification_id;
  return <div role="group" aria-label={`Connection request from ${row.name}`} className="flex items-center gap-1.5">
    <button type="button" disabled={busy || !actions.onDecide} className={COMPACT_ACCENT} onClick={() => actions.onDecide?.(row, id, true)}>Approve</button>
    <button type="button" disabled={busy || !actions.onDecide} className={COMPACT_GHOST} onClick={() => actions.onDecide?.(row, id, false)}>Decline</button>
  </div>;
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

function DisconnectControl({ name, instanceId, confirm, busy, onDisconnect }: {
  name: string;
  instanceId: string;
  confirm: boolean;
  busy: boolean;
  onDisconnect: (instanceId: string) => void;
}) {
  const [asking, setAsking] = useState(false);
  if (asking) {
    return (
      <div role="group" aria-label={`Confirm disconnect ${name}`} className="flex flex-col gap-1.5">
        <span className={UI_TEXT.meta}>This connection is not in your account. Once removed, it cannot be restored from here.</span>
        <div className="flex items-center gap-1.5">
          <button type="button" onClick={() => onDisconnect(instanceId)} disabled={busy} aria-busy={busy} className={`${UI_BUTTON.danger} ${UI_BUTTON.compact}`} autoFocus>
            {busy ? "Disconnecting…" : "Disconnect"}
          </button>
          <button type="button" onClick={() => setAsking(false)} disabled={busy} className={COMPACT_GHOST}>Cancel</button>
        </div>
      </div>
    );
  }
  return (
    <button type="button" onClick={() => confirm ? setAsking(true) : onDisconnect(instanceId)} disabled={busy} aria-busy={busy}
      aria-label={`Disconnect ${name}`} className={COMPACT_GHOST}>
      {busy ? <><BusyState inline hideLabel label="Disconnecting" phase="work" dotSize="micro" /> Disconnecting…</> : "Disconnect"}
    </button>
  );
}

/** An account machine that is not drawn as a paired card: name, state and the one action that moves it forward. */
export function AccountMachineCard({ node, busy, now, actions, hint }: { node: AccountMachineNode; busy: boolean; now: number; actions: MachineNodeActions; hint?: ReactNode }) {
  const reasonId = useId();
  const lock = meshMachineLockedView(node.row);
  const action = node.action;
  const seen = node.online || node.lastSeenAt === null ? null : formatAge(node.lastSeenAt * 1000, now);
  const solid = node.tone === "ok" || node.tone === "progress";
  const retry = ["declined", "link_expired", "link_failed"].includes(node.row.status);
  return (
    <section aria-label={node.name} data-status={node.row.status}
      className={`${NODE_FRAME} border ${solid ? "border-[var(--ct-border)]" : "border-dashed border-[var(--ct-border)]"}`}
      style={{ width: MACHINE_CARD_WIDTH }}>
      {node.callout && <Callout callout={node.callout} onDismiss={() => actions.onDismiss(node.key)}
        onRetry={action?.kind === "connect" ? () => actions.onConnect(node.row, action.target) : undefined} />}
      <div className="flex items-center gap-2 min-w-0">
        <Laptop size={15} className="shrink-0 text-[var(--ct-muted-fg)]" aria-hidden="true" />
        <span className={`min-w-0 flex flex-1 text-[14px] font-medium text-[var(--ct-fg)] ${lockedDimClass(lock)}`}>
          <MachineName name={node.name} onRename={actions.onRename ? (name) => actions.onRename!(node.row, name) : undefined}
            disabledReason={!node.row.target || !node.row.pairInstanceId || !node.row.online ? "Connect to rename it, or rename it on that machine" : undefined} />
        </span>
        <LockBadge view={lock} />
        <SettingsPill tone={PILL_TONE[node.tone]}>{node.status}</SettingsPill>
      </div>
      <div className="text-[12px] text-[var(--ct-muted-fg)]">{node.detail}</div>
      {seen && <div className={UI_TEXT.metaMuted}>Last seen {seen}</div>}
      {hint}
      <SessionControls row={node.row} busy={busy} actions={actions} />
      {(action || node.disconnect) && (
        <div className="flex flex-wrap items-center gap-1.5 pt-0.5">
          {action?.kind === "connect" && (
            <button type="button" onClick={() => actions.onConnect(node.row, action.target)}
              disabled={action.connecting || action.disabledReason !== null} aria-busy={action.connecting}
              aria-describedby={action.disabledReason ? reasonId : undefined}
              aria-label={`${retry ? "Try again to connect" : "Connect"} ${node.name}`} className={`${COMPACT_ACCENT} disabled:opacity-50`}>
              {action.connecting ? <><BusyState inline hideLabel label="Connecting" phase="work" dotSize="micro" /> Connecting…</> : retry ? "Try again" : "Connect"}
            </button>
          )}
          {action?.kind === "plans" && (
            <button type="button" onClick={() => actions.onPlans(node.key)} aria-label={`See plans for ${node.name}`} className={COMPACT_ACCENT}>
              See plans
            </button>
          )}
          {action?.kind === "approve" && <ApprovalControls row={node.row} busy={busy} actions={actions} />}
          {node.disconnect && <DisconnectControl name={node.name} instanceId={node.disconnect.instanceId} confirm={node.disconnect.confirm}
            busy={busy} onDisconnect={(instanceId) => actions.onDisconnect(node.row, instanceId)} />}
        </div>
      )}
      {action?.kind === "connect" && action.disabledReason && <div id={reasonId} className={UI_TEXT.metaMuted}>{action.disabledReason}</div>}
    </section>
  );
}

/** Account state and actions under a paired machine's card. */
export function PeerAccountFooterView({ footer, name, lastSeen, busy, actions, hint }: {
  footer: PeerAccountFooter;
  name: string;
  /** Shown only while the machine is offline. */
  lastSeen: string | null;
  busy: boolean;
  actions: MachineNodeActions;
  hint?: ReactNode;
}) {
  const lock = meshMachineLockedView(footer.row);
  return (
    <div className="flex flex-col gap-2 border-t border-[var(--ct-border)] px-3.5 py-2.5" onClick={(event) => event.stopPropagation()}>
      {footer.callout && <Callout callout={footer.callout} onDismiss={() => actions.onDismiss(footer.key)} />}
      <div className="flex items-center gap-2">
        <LockBadge view={lock} />
        <SettingsPill tone={PILL_TONE[footer.tone]}>{footer.status}</SettingsPill>
        {lastSeen && <span className={UI_TEXT.metaMuted}>Last seen {lastSeen}</span>}
        <div className="ml-auto flex items-center gap-1.5">
          {footer.plans && (
            <button type="button" onClick={() => actions.onPlans(footer.key)} aria-label={`See plans for ${name}`} className={COMPACT_ACCENT}>See plans</button>
          )}
          {footer.disconnect && <DisconnectControl name={name} instanceId={footer.disconnect.instanceId} confirm={footer.disconnect.confirm}
            busy={busy} onDisconnect={(instanceId) => actions.onDisconnect(footer.row, instanceId)} />}
        </div>
      </div>
      {hint}
      <SessionControls row={footer.row} busy={busy} actions={actions} />
      <ApprovalControls row={footer.row} busy={busy} actions={actions} />
    </div>
  );
}

/** Stands in for the machines when there are none to draw: signed out, empty account, loading or unavailable. */
export function AccountStateCard({ card, signingIn, onSignIn, onRetry }: {
  card: AccountCanvasCard;
  signingIn: boolean;
  onSignIn: () => void;
  onRetry: () => void;
}) {
  const frame = `${NODE_FRAME} border border-dashed border-[var(--ct-border)]`;
  if (card.kind === "loading") {
    return <section aria-label="My machines" aria-busy="true" className={frame} style={{ width: MACHINE_CARD_WIDTH }}>
      <BusyState inline label="Loading your machines" phase="work" className="text-[12px] text-[var(--ct-muted-fg)]" />
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
      <p className="m-0 text-[12px] text-[var(--ct-muted-fg)]">Sign in on each machine with the same account and they appear here, ready to connect.</p>
      <div className="pt-0.5">
        <button type="button" onClick={onSignIn} disabled={signingIn} aria-busy={signingIn} className={`${UI_BUTTON.primary} ${UI_BUTTON.compact}`}>
          {signingIn ? "Signing in…" : "Sign in"}
        </button>
      </div>
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
