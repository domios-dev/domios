import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { RefreshCw } from "lucide-react";
import { UI_BUTTON, UI_TEXT } from "../shared/design";
import { BusySpinner } from "../shared/BusyState";
import { RegionLoader } from "../shared/RegionLoader";
import { SettingsPill } from "../Settings/shared";
import type { MachineRowView, MachineStatus, SelfRowView } from "./accountMachines";
import { LockBadge } from "../billing/LockBadge";
import { lockedDimClass } from "../billing/freeLimitModel";
import { meshMachineLockedView } from "../billing/lockSurfaces";
import type { AccountMeshFailure, MeshTarget } from "./accountMeshClient";
import { useMyMachines, type MyMachinesOptions } from "./useMyMachines";

export interface MyMachinesProps extends MyMachinesOptions {
  onFailure: (operation: string, failure: AccountMeshFailure) => void;
}

type Confirm = { kind: "revoke" | "unpair"; key: string } | null;

const ACTION_BTN = `${UI_BUTTON.ghost} ${UI_BUTTON.compact}`;
const DANGER_BTN = `${UI_BUTTON.danger} ${UI_BUTTON.compact}`;
const PRIMARY_BTN = `${UI_BUTTON.primary} ${UI_BUTTON.compact}`;

const STATUS_PILL: Record<MachineStatus, { label: string; tone: "green" | "red" | "muted" | "accent" | "amber" }> = {
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

  const revoke = async (row: MachineRowView, target: MeshTarget) => {
    if (await machines.revoke(row, target)) setConfirm(null);
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
          onAdd={(target) => void add(row, target)}
          onPlans={() => machines.openPlans(row.key)}
          onAskConfirm={(kind) => setConfirm({ kind, key: row.key })}
          onCancelConfirm={(kind) => cancelConfirm(row.key, kind)}
          onRevoke={(target) => void revoke(row, target)}
          onUnpair={(instanceId) => void unpair(row, instanceId)}
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
        ) : <RegionLoader label="Loading your machines" className="min-h-40" />}
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
        <SelfRow self={view.self} onRename={machines.renameSelf} />
      )}
      {rowList("Your account's machines", view.machines, listFailure ? "Your account's machines could not be listed." : "No other machines in your account yet. Sign in to Domios with this account on another machine.")}
      {view.saved.length > 0 && (
        <>
          <div className={`${UI_TEXT.metaMuted} uppercase`}>Saved pairings</div>
          {rowList("Saved pairings", view.saved, null)}
        </>
      )}
      <Notes notes={view.notes} />
      {enrollmentRefused && (
        <div className="flex gap-2">
          {snapshot?.last_refusal?.reason === "unauthenticated" || snapshot?.last_refusal?.reason === "app-check" ? (
            <button type="button" onClick={() => void signIn()} disabled={signingIn} className={PRIMARY_BTN}>Sign in again</button>
          ) : (
            <button type="button" onClick={() => void retryEnrollment()} className={ACTION_BTN}>Retry enrollment</button>
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

function SelfRow({ self, onRename }: {
  self: SelfRowView;
  onRename: (name: string) => Promise<boolean>;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState(false);
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
    if (!name || saving) return;
    setSaving(true);
    const saved = await onRename(name);
    setSaving(false);
    if (saved) close();
    else input.current?.focus();
  };
  return (
    <div role="group" aria-label={`This machine: ${self.name}`} className="ct-card ct-card--quiet p-3 flex flex-col gap-2">
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2 min-w-0">
          <span className="w-2 h-2 rounded-full bg-[var(--ct-green)] flex-shrink-0" aria-hidden="true" />
          <span className="font-medium text-[var(--ct-fg)] truncate">{self.name}</span>
          <SettingsPill tone="accent">This machine</SettingsPill>
        </div>
        {!editing && (
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
      {editing && (
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
    </div>
  );
}

function MachineRow({ row, busy, confirm, register, onAdd, onPlans, onAskConfirm, onCancelConfirm, onRevoke, onUnpair }: {
  row: MachineRowView;
  busy: boolean;
  confirm: "revoke" | "unpair" | null;
  register: (key: string) => (element: HTMLElement | null) => void;
  onAdd: (target: MeshTarget) => void;
  onPlans: () => void;
  onAskConfirm: (kind: "revoke" | "unpair") => void;
  onCancelConfirm: (kind: "revoke" | "unpair") => void;
  onRevoke: (target: MeshTarget) => void;
  onUnpair: (instanceId: string) => void;
}) {
  const reasonId = useId();
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
            {add && row.target && (
              <button
                type="button"
                ref={register(`${row.key}:add`)}
                onClick={() => row.target && onAdd(row.target)}
                disabled={add.reason !== null}
                aria-busy={linking}
                aria-describedby={add.reason && !linking ? reasonId : undefined}
                aria-label={`Connect ${row.name}`}
                className={`${UI_BUTTON.accent} ${UI_BUTTON.compact} disabled:opacity-50`}
              >
                {linking ? <><BusySpinner size={12} /> Connecting…</> : "Connect"}
              </button>
            )}
            {row.status === "over_limit" && (
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
                Revoke
              </button>
            )}
          </div>
        )}
      </div>
      <div className={UI_TEXT.metaMuted}>{row.detail}</div>
      {add?.reason && !linking && <div id={reasonId} className={UI_TEXT.metaMuted}>{add.reason}</div>}
      {confirm === "revoke" && revoke && (
        <ConfirmStrip
          label={`Confirm revoke ${row.name}`}
          message={`Revoke ${row.name}? It leaves your account and frees its machine slot. It must sign in again to rejoin.`}
          action="Revoke"
          pendingAction="Revoking…"
          busy={busy}
          register={register(`${row.key}:confirm-revoke`)}
          onConfirm={() => onRevoke(revoke)}
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

function ConfirmStrip({ label, message, action, pendingAction, busy, register, onConfirm, onCancel }: {
  label: string;
  message: string;
  action: string;
  pendingAction: string;
  busy: boolean;
  register: (element: HTMLElement | null) => void;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <div role="group" aria-label={label} className="flex items-center justify-between gap-2 pt-1">
      <span className={UI_TEXT.meta}>{message}</span>
      <div className="flex items-center gap-1.5 flex-shrink-0">
        <button type="button" ref={register} onClick={onConfirm} disabled={busy} aria-busy={busy} className={DANGER_BTN} autoFocus>
          {busy ? pendingAction : action}
        </button>
        <button type="button" onClick={onCancel} disabled={busy} className={ACTION_BTN}>Cancel</button>
      </div>
    </div>
  );
}
