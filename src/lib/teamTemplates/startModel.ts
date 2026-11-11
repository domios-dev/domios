import { kindDisplayName } from "../roleKind";
import type { TemplatePreview, TemplateRecall, TemplateStartRequest } from "../transport/teamTemplates";

type ManagerSlot = import("../../../gen/ManagerSlot").ManagerSlot;
type ManagerModelChoice = import("../../../gen/ManagerModelChoice").ManagerModelChoice;
type ModelDecision = import("../../../gen/ModelDecision").ModelDecision;
type PayloadKind = import("../../../gen/PayloadKind").PayloadKind;
type RoleStart = import("../../../gen/RoleStart").RoleStart;
type TemplateInputField = import("../../../gen/TemplateInputField").TemplateInputField;
type TemplatePayloadItem = import("../../../gen/TemplatePayloadItem").TemplatePayloadItem;
type TemplateProvenance = import("../../../gen/TemplateProvenance").TemplateProvenance;
export type TemplateSummary = import("../../../gen/TemplateSummary").TemplateSummary;
export type PackageFormat = import("../../../gen/PackageFormat").PackageFormat;
export type ScrubReport = import("../../../gen/ScrubReport").ScrubReport;

export const MANAGER_SLOTS: readonly ManagerSlot[] = ["orchestrator", "planner", "cartographer", "curator"];

export function templateTitle(summary: Pick<TemplateSummary, "title" | "name">): string {
  return summary.title?.trim() || summary.name;
}

export function managerLabel(slot: string, displayName: string): string {
  const slotName = kindDisplayName(slot);
  return displayName && displayName !== slot && displayName !== slotName ? `${slotName} · ${displayName}` : slotName;
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
  | { kind: "substitute"; model: string }
  | { kind: "wait" };

export interface DecisionNotice { tone: "amber" | "blocked"; text: string; actions: DecisionAction[] }

function substitutes(models: readonly string[]): DecisionAction[] {
  return models.map((model): DecisionAction => ({ kind: "substitute", model }));
}

export function decisionNotice(decision: ModelDecision): DecisionNotice | null {
  const alternatives = (decision.alternatives ?? []).map((option) => option.model);
  const blocked = decision.blocked;
  if (!blocked) {
    return decision.amber && decision.choice
      ? { tone: "amber", text: `Using ${decision.choice.model}, a fallback from its own list`, actions: [] }
      : null;
  }
  switch (blocked.kind) {
    case "absent_harness":
      return { tone: "blocked", text: `${blocked.harness} isn't installed`, actions: [{ kind: "install", harness: blocked.harness }, ...substitutes(alternatives)] };
    case "not_signed_in":
      return { tone: "blocked", text: `${blocked.harness} isn't signed in`, actions: [{ kind: "sign_in", harness: blocked.harness }, ...substitutes(alternatives)] };
    case "quota":
      return { tone: "blocked", text: `${blocked.harness} is out of quota`, actions: [...substitutes(alternatives), { kind: "wait" }] };
    case "unknown_model":
      return { tone: "blocked", text: `${blocked.model} isn't a known model`, actions: substitutes(blocked.suggestions) };
    case "same_as_unavailable":
      return { tone: "blocked", text: `The ${kindDisplayName(blocked.role)}'s model isn't available, so choose one`, actions: substitutes(alternatives) };
    case "preferences_exhausted":
      return { tone: "blocked", text: "None of its models are available here, so choose one", actions: substitutes(alternatives) };
  }
}

/** The quiet tooltip shown when the model cannot honour the requested effort. */
export function effortNote(decision: ModelDecision): string | null {
  const effort = decision.effort;
  if (!effort || effort.requested === effort.applied) return null;
  return effort.reason?.trim() || `Asked for ${effort.requested}; runs at ${effort.applied}`;
}

function durationText(value: string): string {
  const match = /^(\d+)([mh])$/.exec(value.trim());
  if (!match) return value.trim();
  return `${match[1]} ${match[2] === "m" ? "min" : "h"}`;
}

export function roleTiming(start: RoleStart): string | null {
  if (start === "with_team") return null;
  if (start === "on_demand") return "Joins when the Orchestrator needs it";
  const [min, max] = start.schedule.split("..");
  return max ? `Visits every ${durationText(min)} to ${durationText(max)}` : `Visits every ${durationText(min)}`;
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

export function initialInputs(fields: readonly TemplateInputField[]): Record<string, string> {
  return Object.fromEntries(fields.map((field) => [field.key, field.default ?? ""]));
}

export function missingInputs(fields: readonly TemplateInputField[], values: Record<string, string>): string[] {
  return fields.filter((field) => field.required && !values[field.key]?.trim()).map((field) => field.key);
}

/** The one reason Start stays disabled, shown beside it; null when the template can start. */
export function startBlocker(preview: TemplatePreview, inputs: Record<string, string>): string | null {
  const missing = missingInputs(preview.form.inputs ?? [], inputs);
  if (missing.length) return `Fill in ${missing.join(", ")}`;
  const need = preview.where_needed?.needs[0];
  if (need) return need.kind === "missing_harness" ? `Needs ${need.harness} on this machine` : `Needs ${need.tool} on this machine`;
  return null;
}

export function buildStartRequest({ preview, inputs, optOut, managerModels }: {
  preview: TemplatePreview;
  inputs: Record<string, string>;
  optOut: ReadonlySet<string>;
  managerModels: Partial<Record<ManagerSlot, ManagerModelChoice>>;
}): TemplateStartRequest {
  const { name, version } = preview.form.template;
  const filled = Object.fromEntries(Object.entries(inputs).filter(([, value]) => value.trim()));
  return {
    template: { name, version },
    inputs: filled,
    opt_out: [...optOut].sort(),
    ...(preview.consent ? { decision_id: preview.consent.decision_id } : {}),
    ...(Object.keys(managerModels).length ? { manager_models: managerModels } : {}),
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

function resultView(value: Record<string, unknown>): StartErrorView | null {
  if (typeof value.error === "string") return startErrorView(value.error);
  if (value.code === "template_unavailable" && typeof value.message === "string") return { message: value.message, reasons: [], choices: [] };
  const result = value as { status?: unknown; failure?: unknown; choices?: unknown; refusal?: unknown };
  switch (result.status) {
    case "manager_failed": {
      const failure = result.failure as { slot: string; reason: string; choices?: string[] };
      return { message: `The ${kindDisplayName(failure.slot)} couldn't start`, reasons: [failure.reason], choices: failure.choices ?? [] };
    }
    case "needs_choice": {
      const choices = result.choices as { slot: string; label: string }[];
      return { message: "Choose a model first", reasons: choices.map((choice) => managerLabel(choice.slot, choice.label)), choices: [] };
    }
    case "refused": {
      const refusal = result.refusal as { code: string; inputs?: string[]; reasons?: string[]; diagnostics?: { message: string }[]; template?: { name: string } };
      if (refusal.code === "missing_inputs") return { message: "Fill in the missing inputs", reasons: refusal.inputs ?? [], choices: [] };
      if (refusal.code === "consent_required") return { message: "Review this template and use it before starting", reasons: refusal.reasons ?? [], choices: [] };
      if (refusal.code === "not_found") return { message: `${refusal.template?.name ?? "This template"} isn't available`, reasons: [], choices: [] };
      return { message: "This template isn't a valid package", reasons: (refusal.diagnostics ?? []).map((diagnostic) => diagnostic.message), choices: [] };
    }
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

const FILTER_BY_PROVENANCE: Record<TemplateProvenance, GalleryFilter> = { builtin: "builtin", saved: "mine", imported: "friends" };

export function sourceLabel(provenance: TemplateProvenance): string {
  return provenance === "builtin" ? "Built-in" : provenance === "saved" ? "Mine" : "Shared · unverified";
}

const key = (summary: TemplateSummary) => `${summary.name}@${summary.version}`;

/** The best recalled matches lead, then the always-available default, then the rest, filtered by source and search text. */
export function galleryEntries(templates: readonly TemplateSummary[], recall: TemplateRecall | null, fallback: TemplateSummary, filter: GalleryFilter, query: string): TemplateSummary[] {
  const seen = new Set<string>([fallback.name]);
  const matches = (recall?.matches ?? []).filter((summary) => !seen.has(summary.name) && seen.add(summary.name));
  const rest = templates.filter((summary) => !seen.has(summary.name) && !seen.has(key(summary)) && seen.add(key(summary)));
  const ordered = [...matches, fallback, ...rest];
  const needle = query.trim().toLowerCase();
  return ordered.filter((summary) => (filter === "all" || FILTER_BY_PROVENANCE[summary.provenance] === filter)
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

export function scrubRows(report: ScrubReport): { label: string; detail: string; count: number }[] {
  return [
    { label: "Machines", detail: "names dropped", count: report.machines_removed },
    { label: "Paths", detail: "made relative to the project", count: report.paths_generalized },
    { label: "Secret values", detail: "names kept", count: report.secret_values_removed },
    { label: "Keys found in instructions", detail: "turned into a secret ask", count: report.secret_asks_added },
    { label: "Chats and history", detail: "never saved", count: report.history_removed },
  ];
}
