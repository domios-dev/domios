import { isControlCharacterChord, isMac, matchesBindingOn } from "./keybindings";
import type { KeybindingsConfig } from "./types";

/** Every app-level shortcut, with the default chord the daemon config declares for it. */
export const APP_SHORTCUT_DEFAULTS = {
  new_tab: "Ctrl+T",
  close_tab: "Ctrl+W",
  reopen_tab: "Ctrl+Shift+T",
  next_tab: "Ctrl+Tab",
  prev_tab: "Ctrl+Shift+Tab",
  new_workspace: "Ctrl+Shift+N",
  copy: "Ctrl+Shift+C",
  paste: "Ctrl+Shift+V",
  search: "Ctrl+Shift+F",
  zoom_in: "Ctrl+=",
  zoom_out: "Ctrl+-",
  zoom_reset: "Ctrl+0",
  open_settings: "Ctrl+,",
  toggle_sidebar: "Ctrl+B",
} as const satisfies Record<keyof KeybindingsConfig | "toggle_sidebar", string>;

export type AppShortcutAction = keyof typeof APP_SHORTCUT_DEFAULTS;
export const APP_SHORTCUT_ACTIONS = Object.keys(APP_SHORTCUT_DEFAULTS) as AppShortcutAction[];

/** `false` means the handler had nothing to act on, so the key keeps its native meaning. */
export type AppShortcutHandler = () => boolean | void;
export type AppShortcutHandlers = Record<AppShortcutAction, AppShortcutHandler>;
export type AppShortcutBindings = Partial<Record<AppShortcutAction, string>>;

const TEXT_EDITING_ACTIONS: ReadonlySet<AppShortcutAction> = new Set(["copy", "paste"]);

export function appShortcutBinding(action: AppShortcutAction, bindings: AppShortcutBindings): string {
  return bindings[action]?.trim() || APP_SHORTCUT_DEFAULTS[action];
}

type ShortcutEvent = Pick<KeyboardEvent, "code" | "ctrlKey" | "metaKey" | "shiftKey" | "altKey" | "target">;

export function resolveAppShortcut(
  event: ShortcutEvent,
  bindings: AppShortcutBindings,
  mac: boolean = isMac,
): AppShortcutAction | null {
  return APP_SHORTCUT_ACTIONS.find((action) => matchesBindingOn(event, appShortcutBinding(action, bindings), mac)) ?? null;
}

function isTerminalInput(target: EventTarget | null): boolean {
  return typeof Element !== "undefined" && target instanceof Element && target.closest(".xterm") !== null;
}

function isTextField(target: EventTarget | null): boolean {
  if (typeof HTMLElement === "undefined" || !(target instanceof HTMLElement)) return false;
  return target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName);
}

/** Whether the focused element owns this chord instead of the app. */
export function shortcutYieldsToTarget(
  action: AppShortcutAction,
  binding: string,
  target: EventTarget | null,
  mac: boolean = isMac,
): boolean {
  const terminal = isTerminalInput(target);
  // Off mac, Ctrl+T/W/B are ^T/^W/^B to the shell or agent TUI running in the terminal.
  if (terminal && !mac && isControlCharacterChord(binding)) return true;
  return TEXT_EDITING_ACTIONS.has(action) && !terminal && isTextField(target);
}

export function dispatchAppShortcut(
  event: ShortcutEvent & Pick<KeyboardEvent, "preventDefault" | "stopPropagation">,
  bindings: AppShortcutBindings,
  handlers: AppShortcutHandlers,
  mac: boolean = isMac,
): AppShortcutAction | null {
  const action = resolveAppShortcut(event, bindings, mac);
  if (!action) return null;
  if (shortcutYieldsToTarget(action, appShortcutBinding(action, bindings), event.target, mac)) return null;
  if (handlers[action]() === false) return null;
  event.preventDefault();
  event.stopPropagation();
  return action;
}

export const FONT_SIZE_MIN = 8;
export const FONT_SIZE_MAX = 28;
export const FONT_SIZE_DEFAULT = 14;

export function zoomedFontSize(current: number, action: "zoom_in" | "zoom_out" | "zoom_reset"): number {
  if (action === "zoom_reset") return FONT_SIZE_DEFAULT;
  const next = current + (action === "zoom_in" ? 1 : -1);
  return Math.min(FONT_SIZE_MAX, Math.max(FONT_SIZE_MIN, next));
}
