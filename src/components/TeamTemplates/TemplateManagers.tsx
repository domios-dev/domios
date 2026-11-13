import { useState, type ReactNode } from "react";
import { ChevronRight, Compass, ListChecks, Map as MapIcon, UserCog, Workflow } from "lucide-react";
import type { ProviderDescriptor } from "../../lib/types";
import { resolveEffort } from "../../lib/newSessionChoice";
import { AgentIcon } from "../AgentIcon";
import { DropdownSelect } from "../shared/DropdownMenu";
import { OrchestrationRoleRow, TeamRosterMember, type OrchestrationPairValues } from "../Sidebar/OrchestrationPairForm";
import { modelEffortText } from "../Sidebar/RoleModelButton";
import { kindDisplayName } from "../../lib/roleKind";
import {
  effortNote, lacksAuthority, managerDecision, managerLabel, managerRoles, pickerSeed, roleSameAs, roleTiming, rowDecisionNotice, splitModel,
  type DecisionAction, type PickerSeed,
} from "../../lib/teamTemplates/startModel";

/** A manager role name: a built-in manager or one of the package's own. */
type ManagerSlot = string;
type ModelDecision = import("../../../gen/ModelDecision").ModelDecision;
type TemplateStartForm = import("../../../gen/TemplateStartForm").TemplateStartForm;
type TemplateRoleSummary = import("../../../gen/TemplateRoleSummary").TemplateRoleSummary;

export type ManagerSeats = Partial<Record<ManagerSlot, PickerSeed>>;
/** Launch-profile controls for the pair rows; other manager seats carry only provider:model. */
export type PairRoleControls = Partial<Record<"orchestrator" | "planner", { modelSlot?: ReactNode; details?: ReactNode }>>;

const SLOT_ICON: Record<ManagerSlot, ReactNode> = {
  orchestrator: <Workflow size={13} aria-hidden="true" />,
  planner: <ListChecks size={13} aria-hidden="true" />,
  cartographer: <MapIcon size={13} aria-hidden="true" />,
  curator: <Compass size={13} aria-hidden="true" />,
};

/** A built-in manager keeps its glyph; every other manager the package registers shares one manager glyph. */
export function managerIcon(role: ManagerSlot): ReactNode {
  return SLOT_ICON[role] ?? <UserCog size={13} aria-hidden="true" />;
}

const ACTION = "rounded border border-[var(--ct-border)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--ct-accent)] hover:brightness-125";
const MUTED_LINE = "text-[10px] text-[var(--ct-muted-fg)]";
const SAME_AS = "same";
const CHOOSE = "choose";

const isPairSlot = (slot: string): slot is "orchestrator" | "planner" => slot === "orchestrator" || slot === "planner";

/** A pair seat is shown only when the package lists that manager to start with the team. */
function startsWithTeam(form: TemplateStartForm, role: string): boolean {
  return form.pickers.some((picker) => picker.role === role)
    || form.roles.some((candidate) => candidate.kind === "manager" && candidate.role === role && candidate.start === "with_team");
}

/** What the role does and the quiet note the host's verdict asks for; workers and built-in managers carry no note. */
function RoleNotes({ role }: { role: TemplateRoleSummary }) {
  return <>
    {role.description && <div className={MUTED_LINE}>{role.description}</div>}
    {lacksAuthority(role) && <div className={MUTED_LINE} data-authority="none">No Domios authority</div>}
  </>;
}

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

function DecisionActionView({ action, harnessName, onAction }: { action: DecisionAction; harnessName: (id: string) => string; onAction: (action: DecisionAction) => void }) {
  const hint = "text-[var(--ct-muted-fg)]";
  switch (action.kind) {
    case "wait": return <span className={hint}>or wait for its quota</span>;
    case "sign_in": return <span className={hint}>Sign in to {harnessName(action.harness)}, or</span>;
    case "connect_tool": return <span className={hint}>Connect {action.tool}, or</span>;
    case "install": return <button type="button" className={ACTION} onClick={() => onAction(action)}>Install {harnessName(action.harness)}</button>;
    case "substitute": return <button type="button" className={ACTION} onClick={() => onAction(action)}>Use {action.model}</button>;
  }
}

function DecisionLines({ decision, providerId, harnessName, onAction }: {
  decision: ModelDecision;
  providerId: string;
  harnessName: (id: string) => string;
  onAction: (action: DecisionAction) => void;
}) {
  const notice = rowDecisionNotice(decision, providerId, harnessName);
  const effort = effortNote(decision);
  if (!notice && !effort) return null;
  return (
    <div className="flex flex-wrap items-center gap-2 text-[10px]" data-decision={notice?.tone ?? "effort"}>
      {notice && <span style={{ color: notice.tone === "amber" ? "var(--ct-yellow,#d97706)" : "var(--ct-red)" }}>{notice.text}</span>}
      {notice?.actions.map((action) => <DecisionActionView key={actionKey(action)} action={action} harnessName={harnessName} onAction={onAction} />)}
      {effort && <span className="text-[var(--ct-muted-fg)]" title={effort} tabIndex={0} aria-label={effort}>Effort adjusted</span>}
    </div>
  );
}

interface RowContext {
  form: TemplateStartForm;
  providers: ProviderDescriptor[];
  disabledProviderIds: ReadonlySet<string>;
  pair: OrchestrationPairValues;
  seats: ManagerSeats;
  onSeatChange: (slot: ManagerSlot, seat: PickerSeed) => void;
  onSeatClear: (slot: ManagerSlot) => void;
  onInstallProvider: (providerId: string) => void;
}

export function harnessNames(providers: readonly ProviderDescriptor[]) {
  return (id: string) => providers.find((provider) => provider.id === id)?.display_name ?? id;
}

/**
 * A manager past the pair. A role following the Orchestrator or Planner (`same_as`) is one select that follows that seat
 * live. A declared model wins; a later role with no model of its own shows the Orchestrator's pick until the owner picks one.
 */
function ManagerSeatRow({ role, context }: { role: TemplateRoleSummary; context: RowContext }) {
  const { form, providers, disabledProviderIds, pair, seats, onSeatChange, onSeatClear, onInstallProvider } = context;
  const slot = role.role as ManagerSlot;
  const label = managerLabel(slot, role.label);
  const harnessName = harnessNames(providers);
  const seatProviders = providers.filter((provider) => !provider.agent_runtime?.launchProfiles);
  const effortFor = (providerId: string, modelId: string, current?: string) =>
    resolveEffort(providers.find((provider) => provider.id === providerId), modelId, current);
  const decision = managerDecision(form.pickers, role);
  const target = roleSameAs(role);
  const followsPair = target !== null && isPairSlot(target) ? target : null;
  const orchestrator = pairSeat("orchestrator", pair);
  const picked = seats[slot];
  const declared = decision ? pickerSeed(decision) : null;
  const waits = role.start !== "with_team";
  const shown = picked ?? declared ?? (waits ? orchestrator : { providerId: "", modelId: "" });
  const timing = roleTiming(role.start);
  if (followsPair && !picked) {
    const followed = pairSeat(followsPair, pair);
    const followedProvider = providers.find((provider) => provider.id === followed.providerId);
    const followedText = modelEffortText(followedProvider, followed.modelId, followed.effortId);
    return (
      <TeamRosterMember role={slot} icon={managerIcon(slot)}>
        <div className="orchestration-role-row flex flex-col gap-1.5" data-testid={`orchestration-role-${label.toLowerCase()}`}>
          <div className="text-[11px] font-medium text-[var(--ct-muted-fg)]">{label}</div>
          <DropdownSelect aria-label={`${label} model`} value={SAME_AS}
            leading={followed.providerId ? <AgentIcon providerId={followed.providerId} className="text-[12px] shrink-0" /> : undefined}
            options={[{ value: SAME_AS, label: `Same as ${kindDisplayName(followsPair)} · ${followedText}` }, { value: CHOOSE, label: "Choose a model…" }]}
            onChange={(value) => { if (value === CHOOSE) onSeatChange(slot, followed); }} />
        </div>
        {decision && <DecisionLines decision={decision} providerId={followed.providerId} harnessName={harnessName} onAction={(action) => {
          if (action.kind === "install") onInstallProvider(action.harness);
          if (action.kind === "substitute") onSeatChange(slot, { ...splitModel(action.model) });
        }} />}
        {timing && <div className={MUTED_LINE}>{timing}</div>}
        <RoleNotes role={role} />
      </TeamRosterMember>
    );
  }
  return (
    <TeamRosterMember role={slot} icon={managerIcon(slot)}>
      <OrchestrationRoleRow label={label} providers={seatProviders} compact
        providerId={shown.providerId} modelId={shown.modelId} effortId={shown.effortId}
        disabledProviderIds={disabledProviderIds} onInstallProvider={onInstallProvider}
        onProviderChange={(providerId) => {
          const modelId = providers.find((provider) => provider.id === providerId)?.default_model ?? "";
          onSeatChange(slot, { providerId, modelId, effortId: effortFor(providerId, modelId) });
        }}
        onModelChange={(modelId) => onSeatChange(slot, { ...shown, modelId, effortId: effortFor(shown.providerId, modelId, shown.effortId) })}
        onEffortChange={(effortId) => onSeatChange(slot, { ...shown, effortId })}
        onSelectionChange={(modelId, effortId) => onSeatChange(slot, { ...shown, modelId, effortId })} />
      {decision && <DecisionLines decision={decision} providerId={shown.providerId} harnessName={harnessName} onAction={(action) => {
        if (action.kind === "install") onInstallProvider(action.harness);
        if (action.kind === "substitute") {
          const next = splitModel(action.model);
          onSeatChange(slot, { ...next, effortId: effortFor(next.providerId, next.modelId) });
        }
      }} />}
      {timing && <div className={MUTED_LINE}>{timing}</div>}
      <RoleNotes role={role} />
      {followsPair && picked && <button type="button" className="self-start text-[10px] text-[var(--ct-accent)] hover:underline" onClick={() => onSeatClear(slot)}>
        Use the {kindDisplayName(followsPair)}'s model
      </button>}
    </TeamRosterMember>
  );
}

/** Managers that join after the start (on demand or on a schedule), collapsed under the pair until the owner opens them. */
export function JoinsLaterGroup({ roles, context }: { roles: TemplateRoleSummary[]; context: RowContext }) {
  const [open, setOpen] = useState(false);
  if (roles.length === 0) return null;
  return <>
    <div className="team-roster-member team-roster-later" role="listitem" data-pair-role="later">
      <span className="team-roster-avatar" aria-hidden="true"><ChevronRight size={13} className={open ? "rotate-90" : ""} /></span>
      <div className="team-roster-body">
        <button type="button" aria-expanded={open} onClick={() => setOpen(!open)}
          className="self-start text-left text-[11px] font-medium text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]">
          Joins later ({roles.length})
        </button>
      </div>
    </div>
    {open && roles.map((role) => <ManagerSeatRow key={role.role} role={role} context={context} />)}
  </>;
}

/** Non-pair managers split into those starting with the team and those joining later. */
function extraManagers(form: TemplateStartForm): { withTeam: TemplateRoleSummary[]; later: TemplateRoleSummary[] } {
  const declared = form.roles.filter((role) => !isPairSlot(role.role));
  const pickerOnly = form.pickers
    .filter((picker) => !isPairSlot(picker.role) && !declared.some((role) => role.role === picker.role))
    .map((picker): TemplateRoleSummary => ({ kind: "manager", role: picker.role, label: picker.label, authority: picker.authority, start: "with_team" }));
  const all = managerRoles([...declared, ...pickerOnly]);
  return { withTeam: all.filter((role) => role.start === "with_team"), later: all.filter((role) => role.start !== "with_team") };
}

/** The default form's managers past the pair: with-team rows, then the collapsed later group. */
export function LaterManagerRows(context: RowContext) {
  const { withTeam, later } = extraManagers(context.form);
  return <>
    {withTeam.map((role) => <ManagerSeatRow key={role.role} role={role} context={context} />)}
    <JoinsLaterGroup roles={later} context={context} />
  </>;
}

/** The template's managers in today's roster rows: the pair, other with-team managers, then the later group. */
export function TemplateManagers({ form, providers, disabledProviderIds, pair, onPairChange, seats, onSeatChange, onSeatClear, onInstallProvider, roleControls }: RowContext & {
  onPairChange: (next: OrchestrationPairValues) => void;
  roleControls?: PairRoleControls;
}) {
  const harnessName = harnessNames(providers);
  const effortFor = (providerId: string, modelId: string, current?: string) =>
    resolveEffort(providers.find((provider) => provider.id === providerId), modelId, current);
  const roles = managerRoles(form.roles);
  const context: RowContext = { form, providers, disabledProviderIds, pair, seats, onSeatChange, onSeatClear, onInstallProvider };
  return (
    <div className="team-roster" role="list" aria-label="Managers" data-testid="template-managers">
      {(["orchestrator", "planner"] as const).filter((slot) => startsWithTeam(form, slot)).map((slot) => {
        const picker = form.pickers.find((candidate) => candidate.role === slot);
        const role = roles.find((candidate) => candidate.role === slot);
        const label = managerLabel(slot, picker?.label ?? role?.label ?? slot);
        const seat = pairSeat(slot, pair);
        const controls = roleControls?.[slot];
        const setSeat = (next: PickerSeed) => onPairChange(withPairSeat(slot, pair, next));
        return (
          <TeamRosterMember key={slot} role={slot} icon={managerIcon(slot)}>
            <OrchestrationRoleRow label={label} providers={providers} compact
              providerId={seat.providerId} modelId={seat.modelId} modelSlot={controls?.modelSlot} details={controls?.details}
              modelControls={!controls?.modelSlot}
              effortId={seat.effortId} disabledProviderIds={disabledProviderIds} onInstallProvider={onInstallProvider}
              onProviderChange={(providerId) => {
                const modelId = providers.find((provider) => provider.id === providerId)?.default_model ?? "";
                setSeat({ providerId, modelId, effortId: effortFor(providerId, modelId) });
              }}
              onModelChange={(modelId) => setSeat({ ...seat, modelId, effortId: effortFor(seat.providerId, modelId, seat.effortId) })}
              onEffortChange={(effortId) => setSeat({ ...seat, effortId })}
              onSelectionChange={(modelId, effortId) => setSeat({ ...seat, modelId, effortId })} />
            {picker && <DecisionLines decision={picker.decision} providerId={seat.providerId} harnessName={harnessName} onAction={(action) => {
              if (action.kind === "install") onInstallProvider(action.harness);
              if (action.kind === "substitute") {
                const next = splitModel(action.model);
                setSeat({ ...next, effortId: effortFor(next.providerId, next.modelId) });
              }
            }} />}
          </TeamRosterMember>
        );
      })}
      <LaterManagerRows {...context} />
    </div>
  );
}
