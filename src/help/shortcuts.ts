import type { AppConfig } from "../lib/types";
import type { HelpAppShortcutId, HelpShortcutSource } from "./types";

const APP_SHORTCUTS: Record<HelpAppShortcutId, string> = {
  command_palette: "Ctrl+K",
  new_tab: "Ctrl+T",
  close_tab: "Ctrl+W",
  new_workspace: "Ctrl+Shift+N",
  settings: "Ctrl+,",
  toggle_sidebar: "Ctrl+B",
};

export function resolveHelpShortcut(source: HelpShortcutSource, config: AppConfig): string {
  if (source.kind === "app") {
    return APP_SHORTCUTS[source.id];
  }
  return config.keybindings[source.key] ?? "";
}
