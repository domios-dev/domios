import type { ReactNode } from "react";
import { Compass, ListChecks, Map as MapIcon, Workflow } from "lucide-react";
import type { ProviderDescriptor } from "../../lib/types";
import { resolveEffort } from "../../lib/newSessionChoice";
import { OrchestrationRoleRow, TeamRosterMember, type OrchestrationPairValues } from "../Sidebar/OrchestrationPairForm";
import {
  effortNote, managerLabel, managerRoles, MANAGER_SLOTS, pickerSeed, roleTiming, rowDecisionNotice, splitModel,
  type DecisionAction, type PickerSeed,
} from "../../lib/teamTemplates/startModel";

type ManagerSlot = import("../../../gen/ManagerSlot").ManagerSlot;
type ManagerModelPicker = import("../../../gen/ManagerModelPicker").ManagerModelPicker;
type TemplateStartForm = import("../../../gen/TemplateStartForm").TemplateStartForm;

export type ManagerSeats = Partial<Record<ManagerSlot, PickerSeed>>;
/** Launch-profile controls for the pair rows; other manager seats carry only provider:model. */
export type PairRoleControls = Partial<Record<"orchestrator" | "planner", { modelSlot?: ReactNode; details?: ReactNode }>>;

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

function actionKey(action: DecisionAction): string {
  switch (action.kind) {
    case "install": case "sign_in": return `${action.kind}:${action.harness}`;
    case "connect_tool": return `${action.kind}:${action.tool}`;
    case "substitute": return `${action.kind}:${action.model}`;
    case "wait": return action.kind;
  }
}

function DecisionActionView({ action, onAction }: { action: DecisionAction; onAction: (action: DecisionAction) => void }) {
  const hint = "text-[var(--ct-muted-fg)]";
  switch (action.kind) {
    case "wait": return <span className={hint}>or wait for its quota</span>;
    case "sign_in": return <span className={hint}>Sign in to {action.harness}, or</span>;
    case "connect_tool": return <span className={hint}>Connect {action.tool}, or</span>;
    case "install": return <button type="button" className={ACTION} onClick={() => onAction(action)}>Install {action.harness}</button>;
    case "substitute": return <button type="button" className={ACTION} onClick={() => onAction(action)}>Use {action.model}</button>;
  }
}

function DecisionLines({ picker, providerId, onAction }: { picker: ManagerModelPicker; providerId: string; onAction: (action: DecisionAction) => void }) {
  const notice = rowDecisionNotice(picker.decision, providerId);
  const effort = effortNote(picker.decision);
  if (!notice && !effort) return null;
  return (
    <div className="flex flex-wrap items-center gap-2 text-[10px]" data-decision={notice?.tone ?? "effort"}>
      {notice && <span style={{ color: notice.tone === "amber" ? "var(--ct-yellow,#d97706)" : "var(--ct-red)" }}>{notice.text}</span>}
      {notice?.actions.map((action) => <DecisionActionView key={actionKey(action)} action={action} onAction={onAction} />)}
      {effort && <span className="text-[var(--ct-muted-fg)]" title={effort} tabIndex={0} aria-label={effort}>Effort adjusted</span>}
    </div>
  );
}

/**
 * The default form's managers past the pair. Each shows what the package decides until the owner picks a model;
 * a picked model is a manager choice, which the caller starts through the template start.
 */
export function LaterManagerRows({ form, providers, disabledProviderIds, pair, seats, onSeatChange, onInstallProvider }: {
  form: TemplateStartForm;
  providers: ProviderDescriptor[];
  disabledProviderIds: ReadonlySet<string>;
  pair: OrchestrationPairValues;
  seats: ManagerSeats;
  onSeatChange: (slot: ManagerSlot, seat: PickerSeed) => void;
  onInstallProvider: (providerId: string) => void;
}) {
  const seatProviders = providers.filter((provider) => !provider.agent_runtime?.launchProfiles);
  const effortFor = (providerId: string, modelId: string, current?: string) =>
    resolveEffort(providers.find((provider) => provider.id === providerId), modelId, current);
  const later = managerRoles(form.roles).filter((role) => role.role !== "orchestrator" && role.role !== "planner");
  return <>{later.map((role) => {
    const slot = role.role as ManagerSlot;
    const decision = form.pickers.find((picker) => picker.slot === slot)?.decision;
    const sameAs = decision?.reason === "same_as";
    const picked = seats[slot];
    const shown = picked ?? (sameAs ? pairSeat("orchestrator", pair) : decision ? pickerSeed(decision) : null) ?? { providerId: "", modelId: "" };
    const note = picked ? null : sameAs ? "Same as Orchestrator" : decision?.choice ? null : "Chosen when it joins, unless you pick one";
    const timing = roleTiming(role.start);
    return (
      <TeamRosterMember key={slot} role={slot} icon={SLOT_ICON[slot] ?? null}>
        <OrchestrationRoleRow label={managerLabel(slot, role.label)} providers={seatProviders}
          providerId={shown.providerId} modelId={shown.modelId} effortId={shown.effortId}
          disabledProviderIds={disabledProviderIds} onInstallProvider={onInstallProvider}
          onProviderChange={(providerId) => {
            const modelId = providers.find((provider) => provider.id === providerId)?.default_model ?? "";
            onSeatChange(slot, { providerId, modelId, effortId: effortFor(providerId, modelId) });
          }}
          onModelChange={(modelId) => onSeatChange(slot, { ...shown, modelId, effortId: effortFor(shown.providerId, modelId, shown.effortId) })}
          onEffortChange={(effortId) => onSeatChange(slot, { ...shown, effortId })} />
        {note && <div className="text-[10px] text-[var(--ct-muted-fg)]">{note}</div>}
        {timing && <div className="text-[10px] text-[var(--ct-muted-fg)]">{timing}</div>}
      </TeamRosterMember>
    );
  })}</>;
}

/** The template's managers in today's roster rows: pickers for with_team slots, a timing line for the rest. */
export function TemplateManagers({ form, providers, disabledProviderIds, pair, onPairChange, seats, onSeatChange, onInstallProvider, roleControls }: {
  form: TemplateStartForm;
  providers: ProviderDescriptor[];
  disabledProviderIds: ReadonlySet<string>;
  pair: OrchestrationPairValues;
  onPairChange: (next: OrchestrationPairValues) => void;
  seats: ManagerSeats;
  onSeatChange: (slot: ManagerSlot, seat: PickerSeed) => void;
  onInstallProvider: (providerId: string) => void;
  roleControls?: PairRoleControls;
}) {
  const seatProviders = providers.filter((provider) => !provider.agent_runtime?.launchProfiles);
  const seatOf = (slot: ManagerSlot): PickerSeed => slot === "orchestrator" || slot === "planner"
    ? pairSeat(slot, pair)
    : seats[slot] ?? { providerId: "", modelId: "" };
  const setSeat = (slot: ManagerSlot, seat: PickerSeed) => {
    if (slot === "orchestrator" || slot === "planner") onPairChange(withPairSeat(slot, pair, seat));
    else onSeatChange(slot, seat);
  };
  const effortFor = (providerId: string, modelId: string, current?: string) =>
    resolveEffort(providers.find((provider) => provider.id === providerId), modelId, current);
  const roles = managerRoles(form.roles);
  const managers = MANAGER_SLOTS.flatMap((slot) => {
    const picker = form.pickers.find((candidate) => candidate.slot === slot);
    const role = roles.find((candidate) => candidate.role === slot);
    return picker || role ? [{ slot, picker, role }] : [];
  });
  return (
    <div className="team-roster" role="list" aria-label="Managers" data-testid="template-managers">
      {managers.map(({ slot, picker, role }) => {
        const label = managerLabel(slot, picker?.label ?? role?.label ?? slot);
        const seat = seatOf(slot);
        const timing = role ? roleTiming(role.start) : null;
        const isPair = slot === "orchestrator" || slot === "planner";
        const controls = isPair ? roleControls?.[slot] : undefined;
        return (
          <TeamRosterMember key={slot} role={slot} icon={SLOT_ICON[slot]}>
            {picker || isPair ? (
              <>
                <OrchestrationRoleRow label={label} providers={isPair ? providers : seatProviders}
                  providerId={seat.providerId} modelId={seat.modelId} modelSlot={controls?.modelSlot} details={controls?.details}
                  modelControls={!controls?.modelSlot}
                  effortId={seat.effortId} disabledProviderIds={disabledProviderIds} onInstallProvider={onInstallProvider}
                  onProviderChange={(providerId) => {
                    const modelId = providers.find((provider) => provider.id === providerId)?.default_model ?? "";
                    setSeat(slot, { providerId, modelId, effortId: effortFor(providerId, modelId) });
                  }}
                  onModelChange={(modelId) => setSeat(slot, { ...seat, modelId, effortId: effortFor(seat.providerId, modelId, seat.effortId) })}
                  onEffortChange={(effortId) => setSeat(slot, { ...seat, effortId })} />
                {picker && <DecisionLines picker={picker} providerId={seat.providerId} onAction={(action) => {
                  if (action.kind === "install") onInstallProvider(action.harness);
                  if (action.kind === "substitute") {
                    const next = splitModel(action.model);
                    setSeat(slot, { ...next, effortId: effortFor(next.providerId, next.modelId) });
                  }
                }} />}
              </>
            ) : (
              <div className="flex flex-col gap-0.5">
                <div className="text-[11px] font-medium text-[var(--ct-muted-fg)]">{label}</div>
                {timing && <div className="text-[10px] text-[var(--ct-muted-fg)]">{timing}</div>}
              </div>
            )}
            {(picker || isPair) && timing && <div className="text-[10px] text-[var(--ct-muted-fg)]">{timing}</div>}
          </TeamRosterMember>
        );
      })}
    </div>
  );
}
