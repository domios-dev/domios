import { clampFloatingPosition } from "../shared/floatingPosition";
import type { PageRect } from "./streamModel";

export const NEEDS_HUMAN_KINDS = ["captcha", "consent", "login", "dialog", "file_chooser"] as const;
export type NeedsHumanKind = (typeof NEEDS_HUMAN_KINDS)[number];

export interface SurfaceState {
  targetId?: string;
  owner: string;
  title?: string;
  url?: string;
  activity?: string | null;
  needsHuman?: NeedsHumanKind | "none" | null;
  profile?: string | null;
  profiles?: string[];
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
  snip?: boolean;
  openExternal?: boolean;
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
  snipping?: boolean;
  state: SurfaceState;
  busy?: ReadonlySet<string>;
}

export interface Invocation {
  method: string;
  args: Record<string, unknown>;
}

export const CHROME_ITEM = { pick: "pick", snip: "snip", openExternal: "open-external", takeControl: "take-control" } as const;

export const CLEAN_PROFILE = "clean";
const ATTACHED_PROFILE = "attached";
export const NEW_PROFILE_OPTION = "__new";
export const PROFILE_NAME_MAX = 40;
const PROFILE_NAME = /^[A-Za-z0-9_-]+$/;

const NEEDS_HUMAN_TEXT: Record<NeedsHumanKind, string> = {
  captcha: "bot check",
  consent: "cookie dialog",
  login: "sign-in",
  dialog: "page dialog",
  file_chooser: "file picker",
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

export function needsHumanNotice(state: SurfaceState): string | null {
  if (!isNeedsHuman(state.needsHuman)) return null;
  const owner = state.owner.trim() || "The agent";
  return `${owner} needs you: ${NEEDS_HUMAN_TEXT[state.needsHuman]}`;
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

const EXTERNAL_PROTOCOLS = new Set(["http:", "https:", "file:"]);

export function canOpenExternally(url: string | undefined): boolean {
  if (!url?.trim()) return false;
  try {
    return EXTERNAL_PROTOCOLS.has(new URL(url).protocol);
  } catch {
    return false;
  }
}

function openExternalItem(context: ToolbarContext): ResolvedToolbarItem {
  return { id: CHROME_ITEM.openExternal, kind: "button", label: "Open in browser", icon: "external-link", disabled: !canOpenExternally(context.state.url) };
}

// Pick and snip are owner annotations, never page input, so neither waits for Take control.
export function resolveToolbar(spec: SurfaceChromeSpec, context: ToolbarContext): ResolvedToolbarItem[] {
  const navigation: ResolvedToolbarItem[] = [];
  const selects: ResolvedToolbarItem[] = [];
  let external = spec.openExternal ? openExternalItem(context) : null;
  for (const item of spec.toolbar) {
    const resolved = resolveDeclared(item, context);
    (resolved.kind === "select" ? selects : navigation).push(resolved);
    if (external && item.source === "url") {
      navigation.push(external);
      external = null;
    }
  }
  if (external) navigation.push(external);
  const chrome: ResolvedToolbarItem[] = [];
  if (spec.pointAndAsk) chrome.push({ id: CHROME_ITEM.pick, kind: "toggle", label: "Pick element", icon: "crosshair", pressed: context.picking, disabled: false });
  if (spec.snip) chrome.push({ id: CHROME_ITEM.snip, kind: "toggle", label: "Snip and ask", icon: "camera", pressed: context.snipping ?? false, disabled: context.busy?.has(CHROME_ITEM.snip) ?? false });
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

export const SNIP_HINT = "Drag to select an area · Esc to cancel";

export function canvasHint(interactive: boolean, picking: boolean, snipping = false): string {
  if (snipping) return SNIP_HINT;
  if (picking) return "Click an element to ask about it. Escape cancels.";
  if (interactive) return "Interacting alongside the agent. Shift+Escape stops.";
  return "Watch only. Press Take control to interact.";
}

export function blockedNotice(owner: string): string {
  return `Press Take control to interact alongside ${owner}.`;
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

export type AskIntent = "question" | "change" | "bug";

export interface GrabAccessibility {
  role?: string | null;
  name?: string | null;
}

export interface GrabTarget {
  react?: string | null;
  selector?: string | null;
  tagName?: string | null;
  text?: string | null;
  accessibility?: GrabAccessibility | null;
}

export interface GrabSelection {
  id: string;
  label: string;
  target: GrabTarget;
}

export type ElementKind = "link" | "button" | "image" | "heading" | "input" | "text";

export interface ElementCard {
  kind: ElementKind;
  name: string;
  path: string;
}

export const ELEMENT_NAME_MAX = 28;

const KIND_WORD: Record<ElementKind, string> = { link: "Link", button: "Button", image: "Image", heading: "Heading", input: "Field", text: "Text" };

const ROLE_KIND: Readonly<Record<string, ElementKind>> = {
  link: "link",
  button: "button",
  menuitem: "button",
  tab: "button",
  image: "image",
  img: "image",
  figure: "image",
  heading: "heading",
  textbox: "input",
  searchbox: "input",
  combobox: "input",
  listbox: "input",
  checkbox: "input",
  radio: "input",
  switch: "input",
  slider: "input",
  spinbutton: "input",
};

const TAG_KIND: Readonly<Record<string, ElementKind>> = {
  a: "link",
  button: "button",
  img: "image",
  svg: "image",
  picture: "image",
  video: "image",
  canvas: "image",
  h1: "heading",
  h2: "heading",
  h3: "heading",
  h4: "heading",
  h5: "heading",
  h6: "heading",
  input: "input",
  textarea: "input",
  select: "input",
};

const NAMELESS_ROLES = new Set(["", "generic", "none", "presentation", "statictext", "inlinetextbox", "paragraph", "rootwebarea", "labeltext"]);

const squash = (value: string | null | undefined) => (value ?? "").replace(/\s+/g, " ").trim();

function quoted(word: string, excerpt: string): string {
  const budget = Math.max(6, ELEMENT_NAME_MAX - word.length - 3);
  const cut = excerpt.length > budget ? `${excerpt.slice(0, budget - 1).trimEnd()}…` : excerpt;
  return `${word} «${cut}»`;
}

function truncate(value: string): string {
  return value.length > ELEMENT_NAME_MAX ? `${value.slice(0, ELEMENT_NAME_MAX - 1).trimEnd()}…` : value;
}

export function elementCard(selection: GrabSelection): ElementCard {
  const { target } = selection;
  const path = [target.react, target.selector].filter((part): part is string => Boolean(part)).join(" · ");
  const tag = squash(target.tagName).toLowerCase();
  const role = squash(target.accessibility?.role).toLowerCase();
  const kind = ROLE_KIND[role] ?? TAG_KIND[tag] ?? "text";
  const accessibleName = squash(target.accessibility?.name);
  if (accessibleName && !NAMELESS_ROLES.has(role)) {
    const word = ROLE_KIND[role] ? KIND_WORD[kind] : role.charAt(0).toUpperCase() + role.slice(1);
    return { kind, name: quoted(word, accessibleName), path };
  }
  const text = squash(target.text) || accessibleName;
  if (text) return { kind, name: quoted(KIND_WORD[kind], text), path };
  return { kind, name: truncate(tag || squash(selection.label) || "element"), path };
}

export function askDialogLabel(card: ElementCard): string {
  return `Ask about ${card.name}`;
}

export function askPlaceholder(owner: string): string {
  return `Ask ${owner}…`;
}

export function grabSendArgs(selection: GrabSelection, comment: string, intent: AskIntent = "question"): { id: string; comment: string; intent: AskIntent } {
  return { id: selection.id, comment: comment.trim(), intent };
}

export interface SnipCapture {
  id: string;
  width: number;
  height: number;
  thumbnail: string | null;
}

export const SNIP_MIN_SIDE = 4;
const SNIP_TITLE_MAX = 60;

export function snipCaptureArgs(rect: PageRect): { x: number; y: number; w: number; h: number } {
  return { x: rect.x, y: rect.y, w: rect.width, h: rect.height };
}

export function snipSendArgs(capture: SnipCapture, comment: string, image: boolean): { id: string; comment: string; image: boolean } {
  return { id: capture.id, comment: comment.trim(), image };
}

export function snipSizeLabel(rect: PageRect): string {
  return `${rect.width} × ${rect.height}`;
}

/** "Area 420×180 at (312, 96) · Title · URL": the one line the snip composer shows. */
export function snipContext(rect: PageRect, state: Pick<SurfaceState, "title" | "url">): string {
  const title = squash(state.title);
  const parts = [
    `Area ${rect.width}×${rect.height} at (${rect.x}, ${rect.y})`,
    title.length > SNIP_TITLE_MAX ? `${title.slice(0, SNIP_TITLE_MAX - 1).trimEnd()}…` : title,
    squash(state.url),
  ];
  return parts.filter(Boolean).join(" · ");
}

export const SNIP_DIALOG_LABEL = "Ask about this area";

export function snipPlaceholder(owner: string): string {
  return `Ask ${owner} about this area`;
}

export function withTranscripts(comment: string, transcripts: readonly string[]): string {
  return [comment, ...transcripts].map((part) => part.trim()).filter(Boolean).join("\n\n");
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
