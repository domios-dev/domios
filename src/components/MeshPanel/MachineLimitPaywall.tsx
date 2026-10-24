import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useState } from "react";
import { LockBadge } from "../billing/LockBadge";
import { meshMachineLockedView } from "../billing/lockSurfaces";
import { UI_BUTTON, UI_TEXT } from "../shared/design";
import { MachineManagement } from "./AddPeerPanel";
import type { MyMachines } from "./useMyMachines";
import { failureMessage } from "./accountMachines";

export function MachineLimitPaywall({ machines }: { machines: MyMachines }) {
  const [managing, setManaging] = useState(false);
  const { view, snapshot, busy } = machines;
  if (view.kind !== "account" || !view.machineLimit) return null;
  const { limit } = view.machineLimit;
  const active = snapshot?.registry?.rows.filter((row) => row.active && row.eligibility !== "revoked");
  const canJoin = active !== undefined && active.length < limit && !snapshot?.registry?.cursor;
  const failure = machines.failures.get("enrollment")?.failure;
  return <div role="group" aria-label="This machine is outside your plan" className="flex flex-col gap-2 border-t border-[var(--ct-border)] px-3.5 py-2.5" onClick={(event) => event.stopPropagation()}>
    <div className="flex items-start gap-2">
      <LockBadge view={meshMachineLockedView({ status: "over_limit" })} />
      <p role="status" className={`m-0 ${UI_TEXT.meta}`}>
        Your {limit === 2 ? "Free " : ""}plan includes {limit} machines. This one is not in your mesh yet.
      </p>
    </div>
    <div className="flex flex-wrap gap-2">
      <button type="button" onClick={() => machines.openPlans("__self__")} className={`${UI_BUTTON.primary} ${UI_BUTTON.compact}`}>See plans</button>
      <button type="button" onClick={() => setManaging(!managing)} aria-expanded={managing} className={`${UI_BUTTON.ghost} ${UI_BUTTON.compact}`}>Manage machines</button>
      {canJoin && <button type="button" onClick={() => void machines.retryEnrollment()} disabled={busy !== null} aria-busy={busy === "enrollment"} className={`${UI_BUTTON.accent} ${UI_BUTTON.compact}`}>Join mesh</button>}
    </div>
    {failure && <InlineErrorNotice description={{ title: "The machine action could not be completed", cause: failureMessage(failure), raw: JSON.stringify(failure), actions: [] }} />}
    {managing && <MachineManagement machines={machines} />}
  </div>;
}
