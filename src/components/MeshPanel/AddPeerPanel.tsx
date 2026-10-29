import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { MachineSessionControls } from "./MachineSessionControls";
import { RefreshCw } from "lucide-react";
import { UI_BUTTON, UI_TEXT } from "../shared/design";
import { BusySpinner, BusyState } from "../shared/BusyState";
import { Loader } from "../shared/Loader";
import { SettingsPill } from "../Settings/shared";
import type { MachineRowView, MachineStatus, SelfRowView } from "./accountMachines";
import { failureMessage } from "./accountMachines";
import { LockBadge } from "../billing/LockBadge";
import { lockedDimClass } from "../billing/freeLimitModel";
import { meshMachineLockedView } from "../billing/lockSurfaces";
import type { AccountMeshFailure, MeshTarget } from "./accountMeshClient";
import { useMyMachines, type MyMachines as MachinesController, type MyMachinesOptions } from "./useMyMachines";

export interface MyMachinesProps extends MyMachinesOptions {
  onFailure: (operation: string, failure: AccountMeshFailure) => void;
}

type Confirm = { kind: "revoke" | "unpair"; key: string } | null;

const ACTION_BTN = `${UI_BUTTON.ghost} ${UI_BUTTON.compact}`;
const DANGER_BTN = `${UI_BUTTON.danger} ${UI_BUTTON.compact}`;
const PRIMARY_BTN = `${UI_BUTTON.primary} ${UI_BUTTON.compact}`;

const STATUS_PILL: Record<MachineStatus, { label: string; tone: "green" | "red" | "muted" | "accent" | "amber" }> = {
  approval_requested: { label: "Approval requested", tone: "accent" },
  declined: { label: "Declined", tone: "muted" },
  link_expired: { label: "Expired", tone: "amber" },
  link_failed: { label: "Connection failed", tone: "amber" },
  signed_out: { label: "Signed out", tone: "muted" },
  linked: { label: "Connected", tone: "green" },
  linking: { label: "Connecting", tone: "accent" },
  unlinked: { label: "Not connected", tone: "muted" },
  endpoint_missing: { label: "Unavailable", tone: "muted" },
  endpoint_stale: { label: "Stale", tone: "amber" },
  expired: { label: "Expired", tone: "amber" },
  revoked: { label: "Revoked", tone: "red" },
  over_limit: { label: "Over limit", tone: "amber" },
  unverified: { label: "Unverified", tone: "amber" },
  legacy: { label: "Legacy pair", tone: "muted" },
  blocked: { label: "Blocked", tone: "red" },
  other_account: { label: "Other account", tone: "red" },
};

export default function MyMachines(props: MyMachinesProps) {
  const machines = useMyMachines(props);
  return <MachineManagement machines={machines} />;
}

export function MachineManagement({ machines }: { machines: MachinesController }) {
  const { view, snapshot, listFailure, loadFailed, refreshing, signingIn, busy, load, signIn, retryEnrollment } = machines;
  const [confirm, setConfirm] = useState<Confirm>(null);
  const controls = useRef(new Map<string, HTMLElement>());

  const enrollmentRefused = view.kind === "account" && snapshot?.enrollment === "refused";

  const pendingFocus = useRef<string | null>(null);
  useEffect(() => {
    const key = pendingFocus.current;
    const element = key ? controls.current.get(key) : undefined;
    if (!element) return;
    pendingFocus.current = null;
    element.focus();
  });
  const refocus = (key: string) => {
    pendingFocus.current = key;
    controls.current.get(key)?.focus();
  };
  const registerControl = (key: string) => (element: HTMLElement | null) => {
    if (element) controls.current.set(key, element);
    else controls.current.delete(key);
  };

  const add = (row: MachineRowView, target: MeshTarget) => machines.connect(row, target);

  const revoke = async (row: MachineRowView, target: MeshTarget, alsoSignOut: boolean) => {
    if (await machines.revoke(row, target, alsoSignOut)) setConfirm(null);
    else refocus(`${row.key}:confirm-revoke`);
  };

  const unpair = async (row: MachineRowView, instanceId: string) => {
    if (await machines.disconnect(row, instanceId)) setConfirm(null);
    else refocus(`${row.key}:confirm-unpair`);
  };

  const cancelConfirm = (key: string, kind: "revoke" | "unpair") => {
    pendingFocus.current = `${key}:${kind}`;
    setConfirm(null);
  };

  const refresh = (
    <button
      type="button"
      onClick={() => void load()}
      disabled={refreshing}
      aria-label="Refresh My machines"
      aria-busy={refreshing}
      className={`${ACTION_BTN} text-[var(--ct-muted)]`}
    >
      <BusySpinner icon={RefreshCw} size={12} spinning={refreshing} />
      Refresh
    </button>
  );

  const rowList = (label: string, rows: MachineRowView[], empty: ReactNode) => (
    <ul aria-label={label} className="ct-card ct-card--quiet flex flex-col">
      {rows.length === 0 ? <li className={`p-3 ${UI_TEXT.metaMuted}`}>{empty}</li> : rows.map((row) => (
        <MachineRow
          key={row.key}
          row={row}
          busy={busy === row.key}
          confirm={confirm?.key === row.key ? confirm.kind : null}
          register={registerControl}
          onDecision={(approve) => { if (row.linkRequest?.notification_id) void machines.decide(row, row.linkRequest.notification_id, approve); }}
          onAdd={(target) => void add(row, target)}
          onPlans={() => machines.openPlans(row.key)}
          onAskConfirm={(kind) => setConfirm({ kind, key: row.key })}
          onCancelConfirm={(kind) => cancelConfirm(row.key, kind)}
          onRevoke={(target, alsoSignOut) => void revoke(row, target, alsoSignOut)}
          onSignOut={() => machines.revokeSession(row)}
          onUnpair={(instanceId) => void unpair(row, instanceId)}
          failure={machines.failures.get(row.key)?.failure}
        />
      ))}
    </ul>
  );

  if (view.kind === "loading") {
    return (
      <section aria-label="My machines" aria-busy={!loadFailed} className="flex flex-col gap-2">
        {loadFailed ? (
          <div className="flex items-center justify-between gap-2">
            <p role="status" className={UI_TEXT.meta}>My machines could not be loaded.</p>
            {refresh}
          </div>
        ) : <Loader variant="compact" label="Loading your machines" />}
      </section>
    );
  }

  if (view.kind === "sign_in") {
    return (
      <section aria-label="My machines" className="flex flex-col gap-3">
        <div className="ct-card ct-card--quiet flex items-center justify-between gap-3 p-3">
          <p role="status" className={view.blocked ? "text-[var(--ct-fg)] font-medium" : UI_TEXT.meta}>{view.headline}</p>
          <button type="button" onClick={() => void signIn()} disabled={signingIn} aria-busy={signingIn} className={`${PRIMARY_BTN} flex-shrink-0`}>
            {signingIn ? "Signing in…" : "Sign in"}
          </button>
        </div>
        <Notes notes={view.notes} />
        {view.saved.length > 0 && rowList("Saved pairings", view.saved, null)}
      </section>
    );
  }

  return (
    <section aria-label="My machines" className="flex flex-col gap-3">
      <div className="flex items-center justify-between gap-2">
        <div className={`${UI_TEXT.metaMuted} uppercase`}>Your account</div>
        {refresh}
      </div>
      {view.self && (
        <SelfRow self={view.self} onRename={machines.renameSelf} onPlans={() => machines.openPlans(view.self!.installationId)} />
      )}
      {view.machineLimit && !view.self && <div role="group" aria-label="This machine plan limit" className="ct-card ct-card--quiet p-3 flex items-center gap-2">
        <span className="font-medium">This machine</span>
        <LockBadge view={meshMachineLockedView({ status: "over_limit" })} />
        <SettingsPill tone="amber">Over limit</SettingsPill>
        <button type="button" aria-label="See plans for this machine" onClick={() => machines.openPlans("self")} className={`${UI_BUTTON.accent} ${UI_BUTTON.compact}`}>See plans</button>
      </div>}
      {rowList("Your account's machines", view.machines, listFailure ? "Your account's machines could not be listed." : "No other machines in your account yet. Sign in to Domios with this account on another machine.")}
      {view.saved.length > 0 && (
        <>
          <div className={`${UI_TEXT.metaMuted} uppercase`}>Saved pairings</div>
          {rowList("Saved pairings", view.saved, null)}
        </>
      )}
      {view.removed && <p role="status" className={UI_TEXT.metaMuted}>This machine was removed from your mesh</p>}
      <Notes notes={view.notes} />
      {enrollmentRefused && !view.machineLimit && (
        <div className="flex gap-2">
          {view.removed || view.reauthRequired || snapshot?.last_refusal?.reason === "unauthenticated" || snapshot?.last_refusal?.reason === "app-check" ? (
            <button type="button" onClick={() => void signIn()} disabled={signingIn} className={PRIMARY_BTN}>{view.removed ? "Sign in and rejoin" : "Sign in again"}</button>
          ) : (
            <button type="button" onClick={() => void retryEnrollment()} className={ACTION_BTN}>{view.removed ? "Rejoin" : "Retry enrollment"}</button>
          )}
        </div>
      )}
    </section>
  );
}

function Notes({ notes }: { notes: string[] }) {
  if (notes.length === 0) return null;
  return <div className="flex flex-col gap-1">{notes.map((note) => <p key={note} className={UI_TEXT.metaMuted}>{note}</p>)}</div>;
}

function SelfRow({ self, onRename, onPlans }: {
  self: SelfRowView;
  onRename: (name: string) => Promise<boolean>;
  onPlans: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const pending = useRef(false);
  const input = useRef<HTMLInputElement | null>(null);
  const button = useRef<HTMLButtonElement | null>(null);
  const returnFocus = useRef(false);
  useEffect(() => {
    if (editing || !returnFocus.current) return;
    returnFocus.current = false;
    button.current?.focus();
  }, [editing]);
  const close = () => {
    returnFocus.current = true;
    setEditing(false);
  };
  const save = async () => {
    const name = draft.trim();
    if (!name || pending.current) return;
    pending.current = true;
    setSaving(true);
    setFailure(null);
    try {
      if (await onRename(name)) close();
      else input.current?.focus();
    } catch (error) {
      setFailure(error instanceof Error ? error.message : "Could not rename. Try again.");
      input.current?.focus();
    } finally {
      pending.current = false;
      setSaving(false);
    }
  };
  return (
    <div role="group" aria-label={`This machine: ${self.name}`} className="ct-card ct-card--quiet p-3 flex flex-col gap-2">
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2 min-w-0">
          <span className="w-2 h-2 rounded-full bg-[var(--ct-green)] flex-shrink-0" aria-hidden="true" />
          <span className="font-medium text-[var(--ct-fg)] truncate">{self.name}</span>
          <SettingsPill tone="accent">This machine</SettingsPill>
          {self.planLimited && <>
            <LockBadge view={meshMachineLockedView({ status: "over_limit" })} />
            <SettingsPill tone="amber">Over limit</SettingsPill>
          </>}
        </div>
        {!editing && !self.readOnly && (
          <button
            type="button"
            ref={button}
            onClick={() => { setDraft(self.displayName ?? ""); setEditing(true); }}
            className={ACTION_BTN}
          >
            Rename this machine
          </button>
        )}
      </div>
      {self.planLimited && <div className="flex items-center justify-between gap-2">
        <span className={UI_TEXT.metaMuted}>This machine is outside your plan's machine limit.</span>
        <button type="button" aria-label="See plans for this machine" onClick={onPlans} className={`${UI_BUTTON.accent} ${UI_BUTTON.compact}`}>See plans</button>
      </div>}
      {editing && !self.readOnly && (
        <form className="flex items-center gap-2" onSubmit={(event) => { event.preventDefault(); void save(); }}>
          <input
            ref={input}
            aria-label="Machine name"
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => { if (event.key === "Escape") close(); }}
            className="ct-field flex-1"
            maxLength={128}
            autoFocus
          />
          <button type="submit" disabled={saving || !draft.trim()} aria-busy={saving} className={PRIMARY_BTN}>
            {saving ? "Saving…" : "Save"}
          </button>
          <button type="button" onClick={close} disabled={saving} className={ACTION_BTN}>Cancel</button>
        </form>
      )}
      {editing && failure && <InlineErrorNotice error={failure} operation="renaming the machine" />}
    </div>
  );
}

function MachineRow({ row, busy, confirm, register, onDecision, onAdd, onPlans, onAskConfirm, onCancelConfirm, onRevoke, onUnpair, onSignOut, failure }: {
  row: MachineRowView;
  busy: boolean;
  confirm: "revoke" | "unpair" | null;
  register: (key: string) => (element: HTMLElement | null) => void;
  onDecision: (approve: boolean) => void;
  onAdd: (target: MeshTarget) => void;
  onPlans: () => void;
  onAskConfirm: (kind: "revoke" | "unpair") => void;
  onCancelConfirm: (kind: "revoke" | "unpair") => void;
  onRevoke: (target: MeshTarget, alsoSignOut: boolean) => void;
  onSignOut: () => Promise<boolean>;
  onUnpair: (instanceId: string) => void;
  failure?: AccountMeshFailure;
}) {
  const reasonId = useId();
  const [alsoSignOut, setAlsoSignOut] = useState(false);
  const pill = STATUS_PILL[row.status];
  const { add, revoke, unpairInstanceId } = row.controls;
  const linking = row.status === "linking";
  const lock = meshMachineLockedView(row);
  return (
    <li aria-label={row.name} data-status={row.status} className="ct-row p-3 flex flex-col gap-1.5">
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2 min-w-0">
          <span
            aria-hidden="true"
            className={`w-2 h-2 rounded-full flex-shrink-0 ${row.online ? "bg-[var(--ct-green)]" : "border border-[var(--ct-muted)]"}`}
          />
          <span className={`text-sm text-[var(--ct-fg)] truncate ${lockedDimClass(lock)}`}>{row.name}</span>
          <LockBadge view={lock} />
          <SettingsPill tone={pill.tone}>{pill.label}</SettingsPill>
        </div>
        {!confirm && (
          <div className="flex items-center gap-1.5 flex-shrink-0">
            {row.status === "approval_requested" && row.linkRequest?.notification_id && <>
              <button type="button" disabled={busy} className={PRIMARY_BTN} onClick={() => onDecision(true)}>Approve</button>
              <button type="button" disabled={busy} className={ACTION_BTN} onClick={() => onDecision(false)}>Decline</button>
            </>}
            {add && row.target && (
              <button
                type="button"
                ref={register(`${row.key}:add`)}
                onClick={() => row.target && onAdd(row.target)}
                disabled={linking || add.reason !== null}
                aria-busy={linking}
                aria-describedby={add.reason && !linking ? reasonId : undefined}
                aria-label={`Connect ${row.name}`}
                className={`${UI_BUTTON.accent} ${UI_BUTTON.compact} disabled:opacity-50`}
              >
                {linking ? <><BusyState inline hideLabel label="Connecting" phase="work" dotSize="micro" /> Connecting…</> :["declined", "link_expired", "link_failed"].includes(row.status) ? "Try again" : "Connect"}
              </button>
            )}
            {(row.status === "over_limit" || row.controls.plans) && (
              <button type="button" ref={register(`${row.key}:plans`)} onClick={onPlans} aria-label={`See plans for ${row.name}`} className={`${UI_BUTTON.accent} ${UI_BUTTON.compact}`}>
                See plans
              </button>
            )}
            {unpairInstanceId && (
              <button type="button" ref={register(`${row.key}:unpair`)} onClick={() => onAskConfirm("unpair")} aria-label={`Disconnect ${row.name}`} className={ACTION_BTN}>
                Disconnect
              </button>
            )}
            {revoke && (
              <button type="button" ref={register(`${row.key}:revoke`)} onClick={() => onAskConfirm("revoke")} aria-label={`Revoke ${row.name}`} className={`${ACTION_BTN} text-[var(--ct-red)]`}>
                Remove from mesh
              </button>
            )}
          </div>
        )}
      </div>
      {row.canSignOut && <MachineSessionControls name={row.name} busy={busy} onSignOut={onSignOut} />}
      <div className={UI_TEXT.metaMuted}>{row.detail}</div>
      {failure && <InlineErrorNotice description={{ title: "The machine action could not be completed", cause: failureMessage(failure), raw: JSON.stringify(failure), actions: [] }} />}
      {add?.reason && !linking && <div id={reasonId} className={UI_TEXT.metaMuted}>{add.reason}</div>}
      {confirm === "revoke" && revoke && (
        <ConfirmStrip
          label={`Confirm revoke ${row.name}`}
          message={`Remove ${row.name} from mesh? It frees its machine slot. It must sign in again to rejoin.`}
          action="Revoke"
          pendingAction="Revoking…"
          busy={busy}
          register={register(`${row.key}:confirm-revoke`)}
          extra={<label className={UI_TEXT.meta}><input type="checkbox" checked={alsoSignOut} disabled={busy} onChange={(event) => setAlsoSignOut(event.target.checked)} /> Also sign it out</label>}
          onConfirm={() => onRevoke(revoke, alsoSignOut)}
          onCancel={() => onCancelConfirm("revoke")}
        />
      )}
      {confirm === "unpair" && unpairInstanceId && (
        <ConfirmStrip
          label={`Confirm disconnect ${row.name}`}
          message={`Disconnect ${row.name}? This removes the connection on this machine only. Its account slot stays allocated.`}
          action="Disconnect"
          pendingAction="Disconnecting…"
          busy={busy}
          register={register(`${row.key}:confirm-unpair`)}
          onConfirm={() => onUnpair(unpairInstanceId)}
          onCancel={() => onCancelConfirm("unpair")}
        />
      )}
    </li>
  );
}

function ConfirmStrip({ label, message, action, pendingAction, busy, register, onConfirm, onCancel, extra }: {
  label: string;
  message: string;
  extra?: ReactNode;
  action: string;
  pendingAction: string;
  busy: boolean;
  register: (element: HTMLElement | null) => void;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <div role="group" aria-label={label} className="flex items-center justify-between gap-2 pt-1">
      <span className={UI_TEXT.meta}>{message}{extra}</span>
      <div className="flex items-center gap-1.5 flex-shrink-0">
        <button type="button" ref={register} onClick={onConfirm} disabled={busy} aria-busy={busy} className={DANGER_BTN} autoFocus>
          {busy ? pendingAction : action}
        </button>
        <button type="button" onClick={onCancel} disabled={busy} className={ACTION_BTN}>Cancel</button>
      </div>
    </div>
  );
}
