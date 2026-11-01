import { useEffect, useRef, useState } from "react";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { UI_BUTTON, UI_TEXT } from "../shared/design";
import { Loader } from "../shared/Loader";
import { SettingsPill } from "../Settings/shared";
import { LockBadge } from "../billing/LockBadge";
import { meshMachineLockedView } from "../billing/lockSurfaces";
import { MachineSessionControls } from "./MachineSessionControls";
import { useMyMachines, type MyMachines as MachinesController, type MyMachinesOptions } from "./useMyMachines";
import { failureMessage, type SelfRowView } from "./accountMachines";
import type { AccountMeshFailure } from "./accountMeshClient";

export interface MyMachinesProps extends MyMachinesOptions { onFailure: (operation: string, failure: AccountMeshFailure) => void }
const ACTION_BTN = `${UI_BUTTON.ghost} ${UI_BUTTON.compact}`;
const PRIMARY_BTN = `${UI_BUTTON.primary} ${UI_BUTTON.compact}`;
export default function MyMachines(props: MyMachinesProps) {
  return <MachineManagement machines={useMyMachines(props)} />;
}
export function MachineManagement({ machines }: { machines: MachinesController }) {
  const { view } = machines;
  if (view.kind === "loading") return machines.loadFailed
    ? <p role="status">Your machines could not be loaded.</p>
    : <Loader variant="compact" label="Loading your machines" />;
  if (view.kind === "sign_in") return <section aria-label="My machines">
    <p role="status">{view.headline}</p>
    <button type="button" disabled={machines.signingIn} className={PRIMARY_BTN} onClick={() => void machines.signIn()}>Sign in</button>
  </section>;
  return <section aria-label="My machines" className="flex flex-col gap-3">
    <button type="button" disabled={machines.refreshing} className={ACTION_BTN} onClick={() => void machines.load()}>Refresh</button>
    {view.self && <SelfRow self={view.self} onRename={machines.renameSelf} onPlans={() => machines.openPlans(view.self!.installationId)} />}
    {view.machineLimit && !view.self && <button type="button" onClick={() => machines.openPlans("self")} className={ACTION_BTN}>See plans for this machine</button>}
    <ul aria-label="Your account's machines" className="ct-card ct-card--quiet flex flex-col">
      {view.machines.length === 0 && <li>No other machines yet. Sign in with this account on another machine to connect automatically.</li>}
      {view.machines.map(row => {
        const failure = machines.failures.get(row.key);
        return <li key={row.key} className="p-3 flex flex-col gap-2" aria-label={row.name}>
          <span>{row.name}</span><p role="status" className={UI_TEXT.meta}>{row.detail}</p>
          {row.controls.plans && <button type="button" className={ACTION_BTN} onClick={() => machines.openPlans(row.key)}>See plans</button>}
          {(row.canSignOut || row.controls.revoke) && <MachineSessionControls name={row.name} busy={machines.busy === row.key}
            onSignOut={row.canSignOut ? () => machines.revokeSession(row) : undefined}
            onRemove={row.controls.revoke ? alsoSignOut => machines.revoke(row, row.controls.revoke!, alsoSignOut) : undefined} />}
          {failure && <InlineErrorNotice role="alert" description={{ title: `Could not ${failure.operation}`, cause: failureMessage(failure.failure), raw: JSON.stringify(failure.failure), actions: [] }} />}
        </li>;
      })}
    </ul>
    {view.notes.map(note => <p key={note} role="status" className={UI_TEXT.metaMuted}>{note}</p>)}
    {(view.removed || view.reauthRequired) && <button type="button" className={PRIMARY_BTN} onClick={() => void machines.signIn()}>{view.removed ? "Sign in and rejoin" : "Sign in again"}</button>}
  </section>;
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

