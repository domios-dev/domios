import { clampFloatingPosition } from "../shared/floatingPosition";

export const NEEDS_HUMAN_KINDS = ["captcha", "consent", "login", "dialog", "file_chooser"] as const;
export type NeedsHumanKind = (typeof NEEDS_HUMAN_KINDS)[number];

export interface SurfaceState {
  owner: string;
  title?: string;
  url?: string;
  activity?: string | null;
  needsHuman?: NeedsHumanKind | "none" | null;
  profile?: string | null;
  profiles?: string[];
}

export type CapsuleTone = "acting" | "idle" | "needs";

export interface CapsuleModel {
  text: string;
  tone: CapsuleTone;
}

export type ToolbarWhen = "interactive" | "always";

export interface ToolbarInvoke {
  method: string;
  args?: Record<string, unknown>;
  valueArg?: string;
}

export interface ToolbarItemSpec {
  id: string;
  label: string;
  kind: "button" | "input" | "select";
  icon?: string;
  when?: ToolbarWhen;
  placeholder?: string;
  source?: string;
  invoke?: ToolbarInvoke;
}

export interface SurfaceChromeSpec {
  toolbar: ToolbarItemSpec[];
  takeControl?: boolean;
  pointAndAsk?: boolean;
  screenshot?: boolean;
}

export interface SelectOption {
  value: string;
  label: string;
}

interface ResolvedBase {
  id: string;
  label: string;
  disabled: boolean;
}

export type ResolvedToolbarItem =
  | (ResolvedBase & { kind: "button"; icon?: string })
  | (ResolvedBase & { kind: "toggle"; icon?: string; pressed: boolean })
  | (ResolvedBase & { kind: "input"; placeholder?: string; value: string })
  | (ResolvedBase & { kind: "select"; options: SelectOption[]; value: string });

export interface ToolbarContext {
  interactive: boolean;
  picking: boolean;
  state: SurfaceState;
  busy?: ReadonlySet<string>;
}

export interface Invocation {
  method: string;
  args: Record<string, unknown>;
}

export const CHROME_ITEM = { pick: "pick", screenshot: "screenshot", takeControl: "take-control" } as const;

export const CLEAN_PROFILE = "clean";
const ATTACHED_PROFILE = "attached";
export const NEW_PROFILE_OPTION = "__new";
export const PROFILE_NAME_MAX = 40;
const PROFILE_NAME = /^[A-Za-z0-9_-]+$/;

const NEEDS_HUMAN_TEXT: Record<NeedsHumanKind, string> = {
  captcha: "needs you: bot check",
  consent: "needs you: cookie dialog",
  login: "needs you: login",
  dialog: "needs you: page dialog",
  file_chooser: "needs you: file picker",
};

export function isNeedsHuman(value: unknown): value is NeedsHumanKind {
  return typeof value === "string" && (NEEDS_HUMAN_KINDS as readonly string[]).includes(value);
}

export function surfaceLabel(owner: string, title: string): string {
  const name = owner.trim();
  return name ? `${name}'s ${title}` : title;
}

export function liveViewLabel(label: string, pageTitle?: string): string {
  const page = pageTitle?.trim();
  return page ? `Live view of ${label}, ${page}` : `Live view of ${label}`;
}

function activeVerb(activity: string | null | undefined): string | null {
  const verb = activity?.trim();
  return verb && verb !== "idle" ? verb : null;
}

function namedProfile(profile: string | null | undefined): string | null {
  return profile && profile !== CLEAN_PROFILE && profile !== ATTACHED_PROFILE ? profile : null;
}

export function capsuleModel(state: SurfaceState): CapsuleModel {
  const parts = [`driven by ${state.owner}`];
  const profile = namedProfile(state.profile);
  if (profile) parts.push(`profile ${profile}`);
  if (isNeedsHuman(state.needsHuman)) {
    parts.push(NEEDS_HUMAN_TEXT[state.needsHuman]);
    return { text: parts.join(" · "), tone: "needs" };
  }
  const verb = activeVerb(state.activity);
  parts.push(verb ?? "idle");
  return { text: parts.join(" · "), tone: verb ? "acting" : "idle" };
}

function enabledWhen(when: ToolbarWhen | undefined, interactive: boolean): boolean {
  return when !== "interactive" || interactive;
}

export function profileOptions(state: SurfaceState): { options: SelectOption[]; value: string; locked: boolean } {
  const names = (state.profiles ?? []).filter((name) => name && name !== CLEAN_PROFILE);
  const options = [
    { value: CLEAN_PROFILE, label: "Clean profile" },
    ...names.map((name) => ({ value: name, label: `Profile: ${name}` })),
    { value: NEW_PROFILE_OPTION, label: "+ New profile…" },
  ];
  const locked = state.profile === ATTACHED_PROFILE;
  const current = state.profile && !locked ? state.profile : CLEAN_PROFILE;
  const value = options.some((option) => option.value === current) ? current : CLEAN_PROFILE;
  return { options, value, locked };
}

function sourceValue(source: string | undefined, state: SurfaceState): string {
  if (source === "url") return state.url ?? "";
  if (source === "title") return state.title ?? "";
  return "";
}

function resolveDeclared(item: ToolbarItemSpec, context: ToolbarContext): ResolvedToolbarItem {
  const busy = context.busy?.has(item.id) ?? false;
  const disabled = busy || !enabledWhen(item.when, context.interactive);
  if (item.kind === "input") {
    return { id: item.id, kind: "input", label: item.label, placeholder: item.placeholder, value: sourceValue(item.source, context.state), disabled };
  }
  if (item.kind === "select") {
    if (item.source === "profiles") {
      const { options, value, locked } = profileOptions(context.state);
      return { id: item.id, kind: "select", label: item.label, options, value, disabled: disabled || locked };
    }
    return { id: item.id, kind: "select", label: item.label, options: [], value: "", disabled: true };
  }
  return { id: item.id, kind: "button", label: item.label, icon: item.icon, disabled };
}

export function resolveToolbar(spec: SurfaceChromeSpec, context: ToolbarContext): ResolvedToolbarItem[] {
  const declared = spec.toolbar.map((item) => resolveDeclared(item, context));
  const navigation = declared.filter((item) => item.kind !== "select");
  const selects = declared.filter((item) => item.kind === "select");
  const chrome: ResolvedToolbarItem[] = [];
  if (spec.pointAndAsk) chrome.push({ id: CHROME_ITEM.pick, kind: "toggle", label: "Pick element", icon: "crosshair", pressed: context.picking, disabled: false });
  if (spec.screenshot) chrome.push({ id: CHROME_ITEM.screenshot, kind: "button", label: "Screenshot", icon: "camera", disabled: context.busy?.has(CHROME_ITEM.screenshot) ?? false });
  const takeControl: ResolvedToolbarItem[] = spec.takeControl
    ? [{ id: CHROME_ITEM.takeControl, kind: "toggle", label: "Take control", pressed: context.interactive, disabled: false }]
    : [];
  return [...navigation, ...chrome, ...selects, ...takeControl];
}

export function toolbarInvocation(item: ToolbarItemSpec, value?: string): Invocation {
  if (!item.invoke) return { method: "action", args: value === undefined ? { id: item.id } : { id: item.id, value } };
  const args = { ...(item.invoke.args ?? {}) };
  if (item.invoke.valueArg && value !== undefined) args[item.invoke.valueArg] = value;
  return { method: item.invoke.method, args };
}

export type SelectChoice = { kind: "new-profile" } | { kind: "value"; value: string };

// The core reads "" as the clean profile.
export function selectChoice(item: Pick<ToolbarItemSpec, "source">, value: string): SelectChoice {
  if (item.source !== "profiles") return { kind: "value", value };
  if (value === NEW_PROFILE_OPTION) return { kind: "new-profile" };
  return { kind: "value", value: value === CLEAN_PROFILE ? "" : value };
}

export function validateProfileName(name: string): string | null {
  const value = name.trim();
  if (!value) return "Enter a profile name";
  if (value.length > PROFILE_NAME_MAX) return `Use at most ${PROFILE_NAME_MAX} characters`;
  if (!PROFILE_NAME.test(value)) return "Use letters, digits, - or _";
  const lower = value.toLowerCase();
  if (lower === CLEAN_PROFILE || lower === ATTACHED_PROFILE) return `"${value}" is reserved`;
  return null;
}

export function profileSwitchedNotice(current: string | null | undefined): string {
  return current && current !== CLEAN_PROFILE ? `Using profile ${current}` : "Using a clean profile";
}

export function withOwner(state: SurfaceState, ownerName: string | null | undefined): SurfaceState {
  const owner = ownerName?.trim();
  return owner && owner !== state.owner ? { ...state, owner } : state;
}

export function addressSubmission(raw: string): string | null {
  const value = raw.trim();
  return value ? value : null;
}

export function canvasHint(interactive: boolean, picking: boolean): string {
  if (picking) return "Click an element to ask about it. Escape cancels.";
  if (interactive) return "Interacting alongside the agent. Shift+Escape stops.";
  return "Watch only. Press Take control to interact.";
}

export function blockedNotice(owner: string): string {
  return `Press Take control to interact alongside ${owner}.`;
}

export function fileNameFromPath(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

export function screenshotSavedNotice(path: string): string {
  return `Saved ${path}`;
}

export function failureNotice(action: string, error: unknown): string {
  const reason = error instanceof Error ? error.message : String(error);
  return `${action} failed: ${reason}`;
}

/** Chromium renders its own error page for network failures, so a toast would only repeat it. */
export function navigationFailureNotice(target: string | undefined, error: unknown): string | null {
  const reason = error instanceof Error ? error.message : String(error);
  if (/net::ERR_/.test(reason)) return null;
  return target ? `Could not open ${target}` : "Navigation failed";
}

export const ASK_INTENTS = [
  { value: "question", label: "Question" },
  { value: "change", label: "Change" },
  { value: "bug", label: "Bug" },
] as const;
export type AskIntent = (typeof ASK_INTENTS)[number]["value"];

export interface GrabTarget {
  react?: string | null;
  selector?: string | null;
}

export interface GrabSelection {
  id: string;
  label: string;
  target: GrabTarget;
}

export interface ElementCard {
  label: string;
  meta: string;
}

export function elementCard(selection: GrabSelection): ElementCard {
  const meta = [selection.target.react, selection.target.selector].filter((part): part is string => Boolean(part)).join(" · ");
  return { label: selection.label, meta };
}

export function askPlaceholder(owner: string): string {
  return `Ask ${owner} about this…`;
}

export function grabSendArgs(selection: GrabSelection, comment: string, intent: AskIntent): { id: string; comment: string; intent: AskIntent } {
  return { id: selection.id, comment: comment.trim(), intent };
}

export interface PopoverAnchor {
  x: number;
  y: number;
  width: number;
  height: number;
}

export const ASK_POPOVER_SIZE = { width: 340, height: 200 } as const;
const ASK_POPOVER_GAP = 12;
const ASK_POPOVER_MARGIN = 8;

export function askPopoverPosition(anchor: PopoverAnchor): { left: number; top: number } {
  const { left, top } = clampFloatingPosition(
    anchor.x + ASK_POPOVER_GAP,
    anchor.y + ASK_POPOVER_GAP,
    ASK_POPOVER_SIZE.width,
    ASK_POPOVER_SIZE.height,
    anchor.width,
    anchor.height,
    ASK_POPOVER_MARGIN,
  );
  return { left, top };
}

export function sentNotice(owner: string): string {
  return `Sent to ${owner}`;
}
