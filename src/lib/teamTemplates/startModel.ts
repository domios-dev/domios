import { humanizeRoleKey, kindDisplayName, roleDisplayLabel } from "../roleKind";
import type { TemplatePreview, TemplateRecall, TemplateStartRequest } from "../transport/teamTemplates";

/** A manager role name: a built-in manager or one of the package's own. */
type ManagerSlot = string;
type ManagerModelChoice = import("../../../gen/ManagerModelChoice").ManagerModelChoice;
type ModelDecision = import("../../../gen/ModelDecision").ModelDecision;
type PayloadKind = import("../../../gen/PayloadKind").PayloadKind;
type RoleStart = import("../../../gen/RoleStart").RoleStart;
type TemplateInputField = import("../../../gen/TemplateInputField").TemplateInputField;
type TemplatePayloadItem = import("../../../gen/TemplatePayloadItem").TemplatePayloadItem;
type TemplateRoleSummary = import("../../../gen/TemplateRoleSummary").TemplateRoleSummary;
type TemplateStartRefusal = import("../../../gen/TemplateStartRefusal").TemplateStartRefusal;
type TemplateStartResult = import("../../../gen/TemplateStartResult").TemplateStartResult;
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

/** Exactly the package's managers, built-in slots first and then the package's own in package order; nothing is implied. */
export function managerRoles(roles: readonly TemplateRoleSummary[]): TemplateRoleSummary[] {
  const rank = (role: TemplateRoleSummary) => {
    const index = MANAGER_SLOTS.indexOf(role.role as ManagerSlot);
    return index < 0 ? MANAGER_SLOTS.length : index;
  };
  return roles.filter((role) => role.kind === "manager").sort((a, b) => rank(a) - rank(b));
}

/** The host's verdict that a role carries no Domios authority: a custom manager. */
export function lacksAuthority(role: { kind?: TemplateRoleSummary["kind"]; authority: TemplateRoleSummary["authority"] }): boolean {
  return role.kind !== "worker" && role.authority === "none";
}

/** The role whose model this one follows (`model: {same_as: <role>}`), named even while that model is undecided. */
export function roleSameAs(role: Pick<TemplateRoleSummary, "same_as">): string | null {
  return role.same_as?.trim() || null;
}

/** A manager's decision: a with_team manager's picker, else the decision its role summary carries. */
export function managerDecision(pickers: readonly { role: string; decision: ModelDecision }[], role: TemplateRoleSummary): ModelDecision | null {
  return pickers.find((picker) => picker.role === role.role)?.decision ?? role.model ?? null;
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

const durationUnit = (minutes: number) => minutes % 60 === 0 ? "h" : "min";
const durationAmount = (minutes: number) => minutes % 60 === 0 ? minutes / 60 : minutes;

/**
 * Smallest first. Both ends in whole hours read "1 to 2 h"; a longest end in minutes keeps both in minutes ("60 to 90 min");
 * only a short start before whole hours mixes units ("45 min to 2 h").
 */
function scheduleText(values: readonly string[]): string {
  const minutes = values.map(durationMinutes);
  if (minutes.some((value) => value === null)) return values.map((value) => value.trim()).join(" to ");
  const sorted = (minutes as number[]).sort((a, b) => a - b);
  const longest = sorted[sorted.length - 1];
  if (sorted.every((value) => durationUnit(value) === "h")) return `${sorted.map(durationAmount).join(" to ")} h`;
  if (durationUnit(longest) === "min") return `${sorted.join(" to ")} min`;
  return sorted.map((value) => `${durationAmount(value)} ${durationUnit(value)}`).join(" to ");
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

type TemplateAsk = import("../../../gen/TemplateAsk").TemplateAsk;
export type ShownAsk = Exclude<TemplateAsk, { kind: "script" }>;

/** The asks a list shows: a script ask is already the skill's flag under Brings, so it is not repeated there. */
export function visibleAsks(asks: readonly TemplateAsk[]): ShownAsk[] {
  return asks.filter((ask): ask is ShownAsk => ask.kind !== "script");
}

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

type MemoryKind = import("../../../gen/MemoryKind").MemoryKind;
type PayloadText = Pick<TemplatePayloadItem, "kind" | "label" | "description" | "memory_kind">;

const MEMORY_KIND: Record<MemoryKind, string> = { fact: "Fact", rule: "Rule", learning: "Learning" };

/**
 * A document reads by its description, else its humanized file name, with the path underneath. A memory fact shows its
 * kind underneath, and an automation reads by its description, else its humanized name. A skill keeps its name.
 */
export function payloadItemText(item: PayloadText): { name: string; detail: string | null } {
  const description = item.description?.trim() || null;
  const humanized = humanizeRoleKey(basename(item.label).replace(/\.[^.]+$/, ""));
  switch (item.kind) {
    case "file": return { name: description ?? humanized, detail: item.label };
    case "memory": return { name: item.label, detail: item.memory_kind ? MEMORY_KIND[item.memory_kind] : description };
    case "automation": return { name: description ?? humanized, detail: null };
    case "skill": return { name: item.label, detail: description };
  }
}

/** A skill's script path relative to the skill folder: "source-check/scripts/fetch.py" reads "scripts/fetch.py". */
export function skillScriptPath(item: Pick<TemplatePayloadItem, "id" | "label" | "script_path">): string | null {
  const path = item.script_path;
  if (!path) return null;
  const root = [item.id.replace(/^skill:/, ""), item.label].find((candidate) => candidate && path.startsWith(`${candidate}/`));
  return root ? path.slice(root.length + 1) : path;
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

/** An input's form label: its humanized key, "Question". */
export function inputFieldLabel(key: string): string {
  return humanizeRoleKey(key);
}

/** Every reason Start stays disabled: inputs to fill, secrets to choose, then harnesses and tools this machine lacks. */
/** `workspaces` names a chosen machine still waiting for its workspace choice. */
export interface StartBlocker { inputs: string[]; secrets: string[]; harnesses: string[]; tools: string[]; workspaces: string[] }

/** Where the whole Team starts when it is not this machine: the peer's name and whether a workspace there is chosen. */
export interface RemoteStart { peer: string; workspaceChosen: boolean }

/** Everything that keeps Start disabled, shown beside it in one line; null when the template can start. */
export function startBlocker(preview: TemplatePreview, inputs: Record<string, string>, secretBindings: Record<string, string> = {}, remote: RemoteStart | null = null): StartBlocker | null {
  const blocker: StartBlocker = { inputs: missingInputs(preview.form.inputs ?? [], inputs), secrets: [], harnesses: [], tools: [], workspaces: [] };
  if (remote && !remote.workspaceChosen) blocker.workspaces.push(remote.peer);
  // A peer binds secrets from its own vault and checks its own harnesses there.
  if (!remote) {
    for (const ask of preview.asks ?? []) {
      if (ask.kind === "secret" && ask.required && !secretBindings[ask.name]) blocker.secrets.push(ask.name);
    }
    for (const need of preview.where_needed?.needs ?? []) {
      if (need.kind === "missing_harness") blocker.harnesses.push(need.harness);
      else blocker.tools.push(need.tool);
    }
  }
  return Object.values(blocker).some((list) => list.length > 0) ? blocker : null;
}

function sentenceList(items: readonly string[]): string {
  return items.length > 1 ? `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}` : items[0] ?? "";
}

/** Missing inputs and secrets are the owner's to fill (amber); a missing harness or tool stops the start until it is installed (red). */
export function blockerView(blocker: StartBlocker, harnessName: (id: string) => string): { tone: "amber" | "red"; text: string } {
  const red = blocker.harnesses.length > 0 || blocker.tools.length > 0;
  const parts = [
    blocker.inputs.length ? `fill in ${sentenceList(blocker.inputs.map(inputFieldLabel))}` : null,
    blocker.secrets.length ? `choose a secret for ${sentenceList(blocker.secrets)}` : null,
    blocker.workspaces.length ? `choose a workspace on ${sentenceList(blocker.workspaces)}` : null,
    blocker.harnesses.length ? `install ${sentenceList(blocker.harnesses.map(harnessName))}` : null,
    blocker.tools.length ? `connect ${sentenceList(blocker.tools)}` : null,
  ].filter((part): part is string => part !== null);
  const sentence = `${sentenceList(parts)}${red ? " to start" : ""}.`;
  return { tone: red ? "red" : "amber", text: `${sentence.charAt(0).toUpperCase()}${sentence.slice(1)}` };
}

export function buildStartRequest({ preview, inputs, optOut, managerModels, secretBindings = {} }: {
  preview: TemplatePreview;
  inputs: Record<string, string>;
  optOut: ReadonlySet<string>;
  /** Owner picks by manager role name; only roles with a pick are present. */
  managerModels: Record<string, ManagerModelChoice>;
  secretBindings?: Record<string, string>;
}): TemplateStartRequest {
  const { name, version } = preview.form.template;
  const filled = Object.fromEntries(Object.entries(inputs).filter(([, value]) => value.trim()));
  const bound = Object.fromEntries(Object.entries(secretBindings).filter(([, entry]) => entry));
  return {
    template: { name, version },
    inputs: filled,
    opt_out: [...optOut].sort(),
    ...(preview.consent ? { decision_id: preview.consent.decision_id } : {}),
    ...(Object.keys(managerModels).length ? { manager_models: managerModels } : {}),
    ...(Object.keys(bound).length ? { secret_bindings: bound } : {}),
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

function refusalView(refusal: TemplateStartRefusal): StartErrorView {
  switch (refusal.code) {
    case "missing_inputs": return { message: "Fill in the missing inputs", reasons: refusal.inputs, choices: [] };
    case "consent_required": return { message: "Review this template and use it before starting", reasons: refusal.reasons, choices: [] };
    case "not_found": return { message: `${humanizePackageName(refusal.template.name)} isn't available`, reasons: [], choices: [] };
    case "invalid_package": return { message: "This template isn't a valid package", reasons: refusal.diagnostics.map((diagnostic) => diagnostic.message), choices: [] };
    case "schedule_unsupported_role":
      return { message: "This template can't start", reasons: [`${roleDisplayLabel(refusal.role, refusal.role)} runs on a schedule, which Domios doesn't run for this role`], choices: [] };
    case "host_owned_role": return { message: `${refusal.role} is a Domios role and can't be part of a template`, reasons: [], choices: [] };
  }
}

/** The typed outcome of a template start that did not start the Team. */
export function startResultView(result: TemplateStartResult): StartErrorView | null {
  switch (result.status) {
    case "started": return null;
    case "manager_failed":
      return { message: `The ${kindDisplayName(result.failure.role)} couldn't start`, reasons: [result.failure.reason], choices: result.failure.choices ?? [] };
    case "needs_choice":
      return { message: "Choose a model first", reasons: result.choices.map((choice) => managerLabel(choice.role, choice.label)), choices: [] };
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

/** A typed start refusal carried by an error, shown as a blocked start; null for any other failure. */
export function startRefusalView(error: unknown): StartErrorView | null {
  const parsed = embeddedJson(error instanceof Error ? error.message : String(error));
  const value = parsed && typeof parsed === "object" ? parsed as Partial<TemplateStartResult> : null;
  return value?.status === "refused" && value.refusal ? refusalView(value.refusal) : null;
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

export type GalleryFilter = "all" | "builtin" | "mine" | "friends" | "channels";

export interface TemplateSource {
  filter: GalleryFilter;
  label: string;
  /** Imported or from a community channel and not yet used through a recorded consent for this exact package. */
  unverified: boolean;
  /** The owner's own package: it can be shared. */
  owned: boolean;
  /** The owner's own package, a built-in or a channel's: it can be exported. */
  exportable: boolean;
  /** A package the owner keeps in My templates; channel and built-in packages leave only with their source. */
  deletable: boolean;
}

export type TemplateOrigin = Pick<TemplateSummary, "provenance" | "received_from" | "verified" | "channel">;

/** An owner's own import (file or CLI) is Mine; a package received over Neighborhood is From friends. Both stay unverified until consented. */
/** The server resolves `received_from` from the friendship and `channel` from the catalog, so a client cannot claim either. */
/** Official domios-teams packages are builtin; a community channel's read as that channel, named by `channelLabel` when known. */
export function templateSource(origin: TemplateOrigin, channelLabel?: string | null): TemplateSource {
  if (origin.provenance === "builtin") return { filter: "builtin", label: "Built-in", unverified: false, owned: false, exportable: true, deletable: false };
  if (origin.provenance === "saved") return { filter: "mine", label: "Mine", unverified: false, owned: true, exportable: true, deletable: true };
  const unverified = !origin.verified;
  if (origin.provenance === "channel") {
    const from = `From ${channelLabel?.trim() || origin.channel?.trim() || "a channel"}`;
    return { filter: "channels", label: unverified ? `${from} · unverified` : from, unverified, owned: false, exportable: true, deletable: false };
  }
  const sender = origin.received_from?.trim();
  // Until its first Use a file is a source like any other; afterwards the owner's own import is simply Mine.
  const label = unverified ? `From ${sender || "file"} · unverified` : sender ? `From ${sender}` : "Mine";
  return { filter: sender ? "friends" : "mine", label, unverified, owned: !sender, exportable: !sender, deletable: true };
}

/** Who stands behind a package the owner is asked to consent to, as the opening of the review's note. */
export function consentOrigin(origin: TemplateOrigin, channelLabel?: string | null): string {
  if (origin.provenance === "channel") return `This team comes from the ${channelLabel?.trim() || origin.channel?.trim() || "community"} channel`;
  const sender = origin.received_from?.trim();
  return sender ? `${sender} shared this team` : "This team came from a file";
}

/** A quiet About fact when the official channel carries a newer built-in than this app bundles; null otherwise. */
export function templateUpdateFact(summary: Pick<TemplateSummary, "provenance" | "version" | "bundled_version">): string | null {
  const bundled = summary.bundled_version?.trim();
  if (summary.provenance !== "builtin" || !bundled || bundled === summary.version) return null;
  return `Updated to ${summary.version} from ${bundled}`;
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
