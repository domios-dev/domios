import type { ReactNode } from "react";
import { Compass, ListChecks, Map as MapIcon, Workflow } from "lucide-react";
import type { ProviderDescriptor } from "../../lib/types";
import { resolveEffort } from "../../lib/newSessionChoice";
import { OrchestrationRoleRow, TeamRosterMember, type OrchestrationPairValues } from "../Sidebar/OrchestrationPairForm";
import {
  decisionNotice, effortNote, managerLabel, MANAGER_SLOTS, roleTiming, splitModel,
  type DecisionAction, type PickerSeed,
} from "../../lib/teamTemplates/startModel";

type ManagerSlot = import("../../../gen/ManagerSlot").ManagerSlot;
type ManagerModelPicker = import("../../../gen/ManagerModelPicker").ManagerModelPicker;
type TemplateStartForm = import("../../../gen/TemplateStartForm").TemplateStartForm;

export type ManagerSeats = Partial<Record<ManagerSlot, PickerSeed>>;

const SLOT_ICON: Record<ManagerSlot, ReactNode> = {
  orchestrator: <Workflow size={13} aria-hidden="true" />,
  planner: <ListChecks size={13} aria-hidden="true" />,
  cartographer: <MapIcon size={13} aria-hidden="true" />,
  curator: <Compass size={13} aria-hidden="true" />,
};

const ACTION = "rounded border border-[var(--ct-border)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--ct-accent)] hover:brightness-125";

function pairSeat(slot: "orchestrator" | "planner", values: OrchestrationPairValues): PickerSeed {
  return slot === "orchestrator"
    ? { providerId: values.orchestratorProviderId, modelId: values.orchestratorModelId, effortId: values.orchestratorEffortId }
    : { providerId: values.plannerProviderId, modelId: values.plannerModelId, effortId: values.plannerEffortId };
}

function withPairSeat(slot: "orchestrator" | "planner", values: OrchestrationPairValues, seat: PickerSeed): OrchestrationPairValues {
  return slot === "orchestrator"
    ? { ...values, orchestratorProviderId: seat.providerId, orchestratorModelId: seat.modelId, orchestratorEffortId: seat.effortId }
    : { ...values, plannerProviderId: seat.providerId, plannerModelId: seat.modelId, plannerEffortId: seat.effortId };
}

function DecisionLines({ picker, onAction }: { picker: ManagerModelPicker; onAction: (action: DecisionAction) => void }) {
  const notice = decisionNotice(picker.decision);
  const effort = effortNote(picker.decision);
  if (!notice && !effort) return null;
  return (
    <div className="flex flex-wrap items-center gap-2 text-[10px]" data-decision={notice?.tone ?? "effort"}>
      {notice && <span style={{ color: notice.tone === "amber" ? "var(--ct-yellow,#d97706)" : "var(--ct-red)" }}>{notice.text}</span>}
      {notice?.actions.map((action) => action.kind === "wait"
        ? <span key="wait" className="text-[var(--ct-muted-fg)]">or wait for its quota</span>
        : action.kind === "sign_in"
          ? <span key="sign-in" className="text-[var(--ct-muted-fg)]">Sign in to {action.harness}, or</span>
          : <button key={`${action.kind}:${"model" in action ? action.model : action.harness}`} type="button" className={ACTION} onClick={() => onAction(action)}>
            {action.kind === "install" ? `Install ${action.harness}` : `Use ${action.model}`}
          </button>)}
      {effort && <span className="text-[var(--ct-muted-fg)]" title={effort} tabIndex={0} aria-label={effort}>Effort adjusted</span>}
    </div>
  );
}

/** The template's managers in today's roster rows: pickers for with_team slots, a timing line for the rest. */
export function TemplateManagers({ form, providers, disabledProviderIds, pair, onPairChange, seats, onSeatChange, onInstallProvider }: {
  form: TemplateStartForm;
  providers: ProviderDescriptor[];
  disabledProviderIds: ReadonlySet<string>;
  pair: OrchestrationPairValues;
  onPairChange: (next: OrchestrationPairValues) => void;
  seats: ManagerSeats;
  onSeatChange: (slot: ManagerSlot, seat: PickerSeed) => void;
  onInstallProvider: (providerId: string) => void;
}) {
  const seatOf = (slot: ManagerSlot): PickerSeed => slot === "orchestrator" || slot === "planner"
    ? pairSeat(slot, pair)
    : seats[slot] ?? { providerId: "", modelId: "" };
  const setSeat = (slot: ManagerSlot, seat: PickerSeed) => {
    if (slot === "orchestrator" || slot === "planner") onPairChange(withPairSeat(slot, pair, seat));
    else onSeatChange(slot, seat);
  };
  const effortFor = (providerId: string, modelId: string, current?: string) =>
    resolveEffort(providers.find((provider) => provider.id === providerId), modelId, current);
  const managers = MANAGER_SLOTS.flatMap((slot) => {
    const picker = form.pickers.find((candidate) => candidate.slot === slot);
    const role = form.roles.find((candidate) => candidate.group === "manager" && candidate.role === slot);
    return picker || role ? [{ slot, picker, role }] : [];
  });
  return (
    <div className="team-roster" role="list" aria-label="Managers" data-testid="template-managers">
      {managers.map(({ slot, picker, role }) => {
        const label = managerLabel(slot, picker?.label ?? role?.label ?? slot);
        const seat = seatOf(slot);
        const timing = role ? roleTiming(role.start) : null;
        return (
          <TeamRosterMember key={slot} role={slot} icon={SLOT_ICON[slot]}>
            {picker ? (
              <>
                <OrchestrationRoleRow label={label} providers={providers} providerId={seat.providerId} modelId={seat.modelId}
                  effortId={seat.effortId} disabledProviderIds={disabledProviderIds} onInstallProvider={onInstallProvider}
                  onProviderChange={(providerId) => {
                    const modelId = providers.find((provider) => provider.id === providerId)?.default_model ?? "";
                    setSeat(slot, { providerId, modelId, effortId: effortFor(providerId, modelId) });
                  }}
                  onModelChange={(modelId) => setSeat(slot, { ...seat, modelId, effortId: effortFor(seat.providerId, modelId, seat.effortId) })}
                  onEffortChange={(effortId) => setSeat(slot, { ...seat, effortId })} />
                <DecisionLines picker={picker} onAction={(action) => {
                  if (action.kind === "install") onInstallProvider(action.harness);
                  if (action.kind === "substitute") {
                    const next = splitModel(action.model);
                    setSeat(slot, { ...next, effortId: effortFor(next.providerId, next.modelId) });
                  }
                }} />
              </>
            ) : (
              <div className="flex flex-col gap-0.5">
                <div className="text-[11px] font-medium text-[var(--ct-muted-fg)]">{label}</div>
                {timing && <div className="text-[10px] text-[var(--ct-muted-fg)]">{timing}</div>}
              </div>
            )}
            {picker && timing && <div className="text-[10px] text-[var(--ct-muted-fg)]">{timing}</div>}
          </TeamRosterMember>
        );
      })}
    </div>
  );
}
