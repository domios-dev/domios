import { humanizeRoleKey, kindDisplayName, roleDisplayLabel } from "../roleKind";
import type { TemplatePreview, TemplateRecall, TemplateStartRequest } from "../transport/teamTemplates";

type ManagerSlot = import("../../../gen/ManagerSlot").ManagerSlot;
type ManagerModelChoice = import("../../../gen/ManagerModelChoice").ManagerModelChoice;
type ModelDecision = import("../../../gen/ModelDecision").ModelDecision;
type PayloadKind = import("../../../gen/PayloadKind").PayloadKind;
type RoleStart = import("../../../gen/RoleStart").RoleStart;
type TemplateInputField = import("../../../gen/TemplateInputField").TemplateInputField;
type TemplatePayloadItem = import("../../../gen/TemplatePayloadItem").TemplatePayloadItem;
type TemplateRoleSummary = import("../../../gen/TemplateRoleSummary").TemplateRoleSummary;
type TemplateStartRefusal = import("../../../gen/TemplateStartRefusal").TemplateStartRefusal;
type TemplateStartResult = import("../../../gen/TemplateStartResult").TemplateStartResult;
type PlacementProblem = import("../../../gen/PlacementProblem").PlacementProblem;
export type TemplateSummary = import("../../../gen/TemplateSummary").TemplateSummary;
export type PackageFormat = import("../../../gen/PackageFormat").PackageFormat;
export type ScrubReport = import("../../../gen/ScrubReport").ScrubReport;

export const MANAGER_SLOTS: readonly ManagerSlot[] = ["orchestrator", "planner", "cartographer", "curator"];

/** A package id's last segment in sentence case, for packages that carry no title; the id itself stays in team.yaml. */
export function humanizePackageName(name: string): string {
  const slug = name.split("/").filter(Boolean).pop() ?? name;
  return humanizeRoleKey(slug.replace(/\.team(\.ya?ml)?$/i, ""));
}

export function templateTitle(summary: Pick<TemplateSummary, "title" | "name">): string {
  return summary.title?.trim() || humanizePackageName(summary.name);
}

/** Effort ids read as words in sentence case: "medium" → "Medium". */
export function effortLabel(effort: string): string {
  const text = effort.trim();
  return text ? `${text.charAt(0).toUpperCase()}${text.slice(1)}` : text;
}

export function managerLabel(slot: string, displayName: string): string {
  const slotName = kindDisplayName(slot);
  const name = roleDisplayLabel(slot, displayName);
  return name !== slotName ? `${slotName} · ${name}` : slotName;
}

export function splitModel(model: string): { providerId: string; modelId: string } {
  const colon = model.indexOf(":");
  return colon < 0 ? { providerId: model, modelId: "" } : { providerId: model.slice(0, colon), modelId: model.slice(colon + 1) };
}

export interface PickerSeed { providerId: string; modelId: string; effortId?: string }

/** A decided model prefills its row; an undecided one leaves the row as today's owner choice. */
export function pickerSeed(decision: ModelDecision): PickerSeed | null {
  if (!decision.choice) return null;
  return { ...splitModel(decision.choice.model), effortId: decision.effort?.applied ?? undefined };
}

export type DecisionAction =
  | { kind: "install"; harness: string }
  | { kind: "sign_in"; harness: string }
  | { kind: "connect_tool"; tool: string }
  | { kind: "substitute"; model: string }
  | { kind: "wait" };

export interface DecisionNotice { tone: "amber" | "blocked"; text: string; actions: DecisionAction[] }

function substitutes(models: readonly string[]): DecisionAction[] {
  return models.map((model): DecisionAction => ({ kind: "substitute", model }));
}

export function decisionNotice(decision: ModelDecision, harnessName: (id: string) => string = (id) => id): DecisionNotice | null {
  const alternatives = (decision.alternatives ?? []).map((option) => option.model);
  const blocked = decision.blocked;
  if (!blocked) {
    return decision.amber && decision.choice
      ? { tone: "amber", text: `Using ${decision.choice.model}, a fallback from its own list`, actions: [] }
      : null;
  }
  switch (blocked.kind) {
    case "absent_harness":
      return { tone: "blocked", text: `${harnessName(blocked.harness)} isn't installed`, actions: [{ kind: "install", harness: blocked.harness }, ...substitutes(alternatives)] };
    case "not_signed_in":
      return { tone: "blocked", text: `${harnessName(blocked.harness)} isn't signed in`, actions: [{ kind: "sign_in", harness: blocked.harness }, ...substitutes(alternatives)] };
    case "quota":
      return { tone: "blocked", text: `${harnessName(blocked.harness)} is out of quota`, actions: [...substitutes(alternatives), { kind: "wait" }] };
    case "unknown_model":
      return { tone: "blocked", text: `${blocked.model} isn't a known model`, actions: substitutes(blocked.suggestions) };
    case "no_reasoning_effort":
      return { tone: "blocked", text: `${blocked.model} has no reasoning effort setting, so choose another model`, actions: substitutes(alternatives) };
    case "missing_tool":
      return { tone: "blocked", text: `${blocked.tool} isn't connected`, actions: [{ kind: "connect_tool", tool: blocked.tool }, ...substitutes(alternatives)] };
    case "same_as_unavailable":
      return { tone: "blocked", text: `The ${kindDisplayName(blocked.role)}'s model isn't available, so choose one`, actions: substitutes(alternatives) };
    case "preferences_exhausted":
      return { tone: "blocked", text: "None of its models are available here, so choose one", actions: substitutes(alternatives) };
  }
}

/** A harness blocker belongs to the row only while that row runs on the blocked harness; another chosen harness is unaffected. */
export function rowDecisionNotice(decision: ModelDecision, rowProviderId: string, harnessName?: (id: string) => string): DecisionNotice | null {
  const blocked = decision.blocked;
  const harness = blocked && "harness" in blocked ? blocked.harness : null;
  if (harness && rowProviderId && rowProviderId !== harness) return null;
  return decisionNotice(decision, harnessName);
}

const PAIR_MANAGERS: readonly TemplateRoleSummary[] = [
  { group: "manager", role: "orchestrator", label: "orchestrator", start: "with_team" },
  { group: "manager", role: "planner", label: "planner", start: "with_team" },
];

/** The managers a start launches, in slot order: every Team starts an Orchestrator and a Planner even when the package omits them. */
export function managerRoles(roles: readonly TemplateRoleSummary[]): TemplateRoleSummary[] {
  const declared = roles.filter((role) => role.group === "manager");
  const all = [...declared, ...PAIR_MANAGERS.filter((pair) => !declared.some((role) => role.role === pair.role))];
  const rank = (role: TemplateRoleSummary) => {
    const index = MANAGER_SLOTS.indexOf(role.role as ManagerSlot);
    return index < 0 ? MANAGER_SLOTS.length : index;
  };
  return all.sort((a, b) => rank(a) - rank(b));
}

/** The quiet tooltip shown when the model cannot honour the requested effort. */
export function effortNote(decision: ModelDecision): string | null {
  const effort = decision.effort;
  if (!effort || effort.requested === effort.applied) return null;
  return effort.reason?.trim() || `Asked for ${effort.requested}; runs at ${effort.applied}`;
}

function durationMinutes(value: string): number | null {
  const match = /^(\d+)([mh])$/.exec(value.trim());
  if (!match) return null;
  return Number(match[1]) * (match[2] === "h" ? 60 : 1);
}

/** Both ends in their smaller common unit, smallest first: "45m..2h" reads "45 to 120 min". */
function scheduleText(values: readonly string[]): string {
  const minutes = values.map(durationMinutes);
  if (minutes.some((value) => value === null)) return values.map((value) => value.trim()).join(" to ");
  const sorted = (minutes as number[]).sort((a, b) => a - b);
  const hours = sorted.every((value) => value % 60 === 0);
  return `${sorted.map((value) => hours ? value / 60 : value).join(" to ")} ${hours ? "h" : "min"}`;
}

export function roleTiming(start: RoleStart): string | null {
  if (start === "with_team") return null;
  if (start === "on_demand") return "Joins when the Orchestrator needs it";
  return `Visits every ${scheduleText(start.schedule.split("..").filter(Boolean))}`;
}

export const PAYLOAD_GROUPS: readonly { kind: PayloadKind; title: string; one: string; many: string }[] = [
  { kind: "file", title: "Documents", one: "document", many: "documents" },
  { kind: "memory", title: "Memory facts", one: "memory fact", many: "memory facts" },
  { kind: "skill", title: "Skills", one: "skill", many: "skills" },
  { kind: "automation", title: "Automations", one: "automation", many: "automations" },
];

export function bringsSummary(payload: readonly TemplatePayloadItem[], optOut: ReadonlySet<string>): string {
  const parts = PAYLOAD_GROUPS.flatMap((group) => {
    const kept = payload.filter((item) => item.kind === group.kind && !optOut.has(item.id)).length;
    return kept === 0 ? [] : [`${kept} ${kept === 1 ? group.one : group.many}`];
  });
  return parts.length ? parts.join(", ") : "Nothing";
}

const ICON_FILE = /(^|\/)icon\.(svg|png)$/i;

/** The package icon is its identity, not something it brings; it stays in the package but out of Brings and Includes. */
export function visiblePayload(payload: readonly TemplatePayloadItem[]): TemplatePayloadItem[] {
  return payload.filter((item) => !(item.kind === "file" && ICON_FILE.test(item.label) && !(item.used_by ?? []).length));
}

function basename(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/** A document reads by its description, else its humanized file name, with the path underneath; other kinds keep their label. */
export function payloadItemText(item: Pick<TemplatePayloadItem, "kind" | "label" | "description">): { name: string; detail: string | null } {
  if (item.kind !== "file") return { name: item.label, detail: item.description?.trim() || null };
  const name = item.description?.trim() || humanizeRoleKey(basename(item.label).replace(/\.[^.]+$/, ""));
  return { name, detail: item.label };
}

/** Whether every item of `items` is kept, none is, or some are; the toggle link turns the group all on or all off. */
export function groupKeptState(items: readonly Pick<TemplatePayloadItem, "id">[], optOut: ReadonlySet<string>): "all" | "none" | "some" {
  const kept = items.filter((item) => !optOut.has(item.id)).length;
  return kept === items.length ? "all" : kept === 0 ? "none" : "some";
}

export function withKept(optOut: ReadonlySet<string>, ids: readonly string[], kept: boolean): ReadonlySet<string> {
  const next = new Set(optOut);
  for (const id of ids) {
    if (kept) next.delete(id); else next.add(id);
  }
  return next;
}

export function initialInputs(fields: readonly TemplateInputField[]): Record<string, string> {
  return Object.fromEntries(fields.map((field) => [field.key, field.default ?? ""]));
}

export function missingInputs(fields: readonly TemplateInputField[], values: Record<string, string>): string[] {
  return fields.filter((field) => field.required && !values[field.key]?.trim()).map((field) => field.key);
}

export type StartBlocker =
  | { kind: "inputs"; keys: string[] }
  | { kind: "secret"; name: string }
  | { kind: "harness"; harness: string }
  | { kind: "tool"; tool: string };

/** The one reason Start stays disabled, shown beside it; null when the template can start. */
export function startBlocker(preview: TemplatePreview, inputs: Record<string, string>, secretBindings: Record<string, string> = {}, remotePeer: string | null = null): StartBlocker | null {
  const missing = missingInputs(preview.form.inputs ?? [], inputs);
  if (missing.length) return { kind: "inputs", keys: missing };
  // A peer binds secrets from its own vault and checks its own harnesses when its owner answers.
  if (remotePeer) return null;
  const unbound = (preview.asks ?? []).find((ask) => ask.kind === "secret" && ask.required && !secretBindings[ask.name]);
  if (unbound?.kind === "secret") return { kind: "secret", name: unbound.name };
  const need = preview.where_needed?.needs[0];
  if (need) return need.kind === "missing_harness" ? { kind: "harness", harness: need.harness } : { kind: "tool", tool: need.tool };
  return null;
}

/** A missing input is the owner's to fill (amber); a missing harness or tool stops the start until it is installed (red). */
export function blockerView(blocker: StartBlocker, harnessName: (id: string) => string): { tone: "amber" | "red"; text: string } {
  switch (blocker.kind) {
    case "inputs": return { tone: "amber", text: `Fill in ${blocker.keys.map((key) => humanizeRoleKey(key).toLowerCase()).join(", ")}.` };
    case "secret": return { tone: "amber", text: `Choose a secret for ${blocker.name}.` };
    case "harness": return { tone: "red", text: `Install ${harnessName(blocker.harness)} to start.` };
    case "tool": return { tone: "red", text: `Connect ${blocker.tool} to start.` };
  }
}

export function buildStartRequest({ preview, inputs, optOut, managerModels, secretBindings = {}, rolePlacements = {} }: {
  preview: TemplatePreview;
  inputs: Record<string, string>;
  optOut: ReadonlySet<string>;
  managerModels: Partial<Record<ManagerSlot, ManagerModelChoice>>;
  secretBindings?: Record<string, string>;
  rolePlacements?: Record<string, string>;
}): TemplateStartRequest {
  const { name, version } = preview.form.template;
  const filled = Object.fromEntries(Object.entries(inputs).filter(([, value]) => value.trim()));
  const bound = Object.fromEntries(Object.entries(secretBindings).filter(([, entry]) => entry));
  const placed = Object.fromEntries(Object.entries(rolePlacements).filter(([, peer]) => peer));
  return {
    template: { name, version },
    inputs: filled,
    opt_out: [...optOut].sort(),
    ...(preview.consent ? { decision_id: preview.consent.decision_id } : {}),
    ...(Object.keys(managerModels).length ? { manager_models: managerModels } : {}),
    ...(Object.keys(bound).length ? { secret_bindings: bound } : {}),
    ...(Object.keys(placed).length ? { role_placements: placed } : {}),
  };
}

export interface StartErrorView { message: string; reasons: string[]; choices: string[] }

function embeddedJson(text: string): unknown {
  const start = text.indexOf("{");
  if (start < 0) return null;
  try {
    return JSON.parse(text.slice(start));
  } catch {
    return null;
  }
}

function placementText(role: string, peer: string, problem: PlacementProblem): string {
  const name = kindDisplayName(role);
  switch (problem) {
    case "unknown_role": return `This template has no ${name} role to run on ${peer}`;
    case "not_a_worker": return `The ${name} is a manager and runs on this machine`;
    case "unknown_peer": return `${peer} isn't a paired machine`;
    case "not_full_access": return `${peer} needs Full access to run the ${name}`;
  }
}

function refusalView(refusal: TemplateStartRefusal): StartErrorView {
  switch (refusal.code) {
    case "missing_inputs": return { message: "Fill in the missing inputs", reasons: refusal.inputs, choices: [] };
    case "consent_required": return { message: "Review this template and use it before starting", reasons: refusal.reasons, choices: [] };
    case "not_found": return { message: `${humanizePackageName(refusal.template.name)} isn't available`, reasons: [], choices: [] };
    case "invalid_package": return { message: "This template isn't a valid package", reasons: refusal.diagnostics.map((diagnostic) => diagnostic.message), choices: [] };
    case "invalid_placement": return { message: placementText(refusal.role, refusal.peer, refusal.problem), reasons: [], choices: [] };
  }
}

/** The typed outcome of a template start that did not start the Team. */
export function startResultView(result: TemplateStartResult): StartErrorView | null {
  switch (result.status) {
    case "started": return null;
    case "manager_failed":
      return { message: `The ${kindDisplayName(result.failure.slot)} couldn't start`, reasons: [result.failure.reason], choices: result.failure.choices ?? [] };
    case "needs_choice":
      return { message: "Choose a model first", reasons: result.choices.map((choice) => managerLabel(choice.slot, choice.label)), choices: [] };
    case "refused": return refusalView(result.refusal);
  }
}

const START_STATUSES: readonly string[] = ["started", "needs_choice", "manager_failed", "refused"];

function resultView(value: Record<string, unknown>): StartErrorView | null {
  if (typeof value.error === "string") return startErrorView(value.error);
  if (typeof value.status === "string" && START_STATUSES.includes(value.status)) return startResultView(value as TemplateStartResult);
  if (typeof value.code === "string" && typeof value.message === "string") {
    const diagnostics = Array.isArray(value.diagnostics) ? value.diagnostics as { message?: unknown }[] : [];
    return { message: value.message, reasons: diagnostics.flatMap((diagnostic) => typeof diagnostic.message === "string" ? [diagnostic.message] : []), choices: [] };
  }
  return null;
}

/** A start failure shown in the dialog, with the server's reasons and choices when it sent a typed result. */
export function startErrorView(error: unknown): StartErrorView {
  const text = error instanceof Error ? error.message : String(error);
  const parsed = embeddedJson(text);
  const typed = parsed && typeof parsed === "object" ? resultView(parsed as Record<string, unknown>) : null;
  if (typed) return typed;
  const unavailable = /template_unavailable:\s*(.+)$/s.exec(text);
  return { message: unavailable ? unavailable[1].trim() : text, reasons: [], choices: [] };
}

export type GalleryFilter = "all" | "builtin" | "mine" | "friends";

export interface TemplateSource {
  filter: GalleryFilter;
  label: string;
  /** Imported and not yet used through a recorded consent. */
  unverified: boolean;
  /** The owner's own package: it can be shared and deleted. */
  owned: boolean;
  /** The owner's own package or a built-in: it can be exported. */
  exportable: boolean;
}

/** An owner's own import (file or CLI) is Mine; a package received over Neighborhood is From friends. Both stay unverified until consented. */
/** The server resolves `received_from` from the friendship, so a client cannot claim a sender. */
export function templateSource(origin: Pick<TemplateSummary, "provenance" | "received_from" | "verified">): TemplateSource {
  if (origin.provenance === "builtin") return { filter: "builtin", label: "Built-in", unverified: false, owned: false, exportable: true };
  if (origin.provenance === "saved") return { filter: "mine", label: "Mine", unverified: false, owned: true, exportable: true };
  const unverified = !origin.verified;
  const sender = origin.received_from?.trim();
  // Until its first Use a file is a source like any other; afterwards the owner's own import is simply Mine.
  const label = unverified ? `From ${sender || "file"} · unverified` : sender ? `From ${sender}` : "Mine";
  return { filter: sender ? "friends" : "mine", label, unverified, owned: !sender, exportable: !sender };
}

const key = (summary: TemplateSummary) => `${summary.name}@${summary.version}`;

/** The best recalled matches lead, then the always-available default, then the rest, filtered by source and search text. */
export function galleryEntries(templates: readonly TemplateSummary[], recall: TemplateRecall | null, fallback: TemplateSummary, filter: GalleryFilter, query: string): TemplateSummary[] {
  const seen = new Set<string>([fallback.name]);
  const matches = (recall?.matches ?? []).filter((summary) => !seen.has(summary.name) && seen.add(summary.name));
  const rest = templates.filter((summary) => !seen.has(summary.name) && !seen.has(key(summary)) && seen.add(key(summary)));
  const ordered = [...matches, fallback, ...rest];
  const needle = query.trim().toLowerCase();
  return ordered.filter((summary) => (filter === "all" || templateSource(summary).filter === filter)
    && (!needle || [summary.name, summary.title ?? "", summary.description, ...(summary.when ?? [])].some((text) => text.toLowerCase().includes(needle))));
}

export interface ExportFormatOption { format: PackageFormat; name: string; filename: string; hint: string; disabledReason?: string }

export function exportFormats(summary: Pick<TemplateSummary, "name" | "icon">): ExportFormatOption[] {
  const slug = summary.name.split("/").pop() ?? summary.name;
  const binary = summary.icon != null && summary.icon.media_type !== "image/svg+xml";
  return [
    { format: "yaml", name: "One file", filename: `${slug}.team.yaml`, hint: "Text only. Easy to paste in a chat or keep in git.",
      disabledReason: binary ? "Its icon is an image file, which can't go in a text file." : undefined },
    { format: "team", name: "Package", filename: `${slug}.team`, hint: "One zip with every file and asset." },
    { format: "folder", name: "Folder", filename: `${slug}/`, hint: "Plain files, for editing in a repo." },
  ];
}

export const DEFAULT_EXPORT_FORMAT: PackageFormat = "team";

function plural(count: number, one: string, many: string): string {
  return `${count} ${count === 1 ? one : many}`;
}

function secretNamesDetail(count: number, names: readonly string[]): string {
  if (names.length === 0) return `${plural(count, "value", "values")} dropped, ${count === 1 ? "its name" : "their names"} kept`;
  return names.length === 1 ? `${names[0]} kept as a name, its value dropped` : `${names.join(", ")} kept as names, their values dropped`;
}

export interface ScrubRow { label: string; detail: string; count: number | null }

/** Only what was actually removed; chats and history are never saved, so that row carries no count. */
export function scrubRows(report: ScrubReport): ScrubRow[] {
  const rows: ScrubRow[] = [
    { label: "Machines", detail: `${plural(report.machines_removed, "machine name", "machine names")} dropped`, count: report.machines_removed },
    { label: "Paths", detail: `${plural(report.paths_generalized, "absolute path", "absolute paths")} made relative to {project}`, count: report.paths_generalized },
    { label: "Secret values", detail: secretNamesDetail(report.secret_values_removed, report.secret_names ?? []), count: report.secret_values_removed },
    { label: "Keys found in instructions", detail: `${plural(report.secret_asks_added, "key", "keys")} turned into a secret ask`, count: report.secret_asks_added },
  ];
  const removed = rows.filter((row) => (row.count ?? 0) > 0);
  return report.history_removed > 0 ? [...removed, { label: "Chats and history", detail: "never saved", count: null }] : removed;
}
