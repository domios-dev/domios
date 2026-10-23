import type { AppConfig } from "../lib/types";
import { appShortcutBinding } from "../lib/appShortcuts";
import type { HelpAppShortcutId, HelpShortcutSource } from "./types";

const APP_SHORTCUTS: Record<HelpAppShortcutId, string> = {
  command_palette: "Ctrl+K",
  toggle_sidebar: appShortcutBinding("toggle_sidebar", {}),
};

/** The chord the app shortcut dispatcher acts on, including its default when config leaves a binding unset. */
export function resolveHelpShortcut(source: HelpShortcutSource, config: AppConfig): string {
  if (source.kind === "app") {
    return APP_SHORTCUTS[source.id];
  }
  return appShortcutBinding(source.key, config.keybindings);
}
