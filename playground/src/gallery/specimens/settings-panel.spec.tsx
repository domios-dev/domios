import { SettingsPanel } from "../../../../src/components/Settings/SettingsPanel";
import type { SettingsLinkId } from "../../../../src/components/Settings/sections";
import type { ShellProfile } from "../../../../src/lib/types";
import { mockCommand } from "../../mocks/backend";
import { appConfig, httpApi } from "../fixtures";
import { defineSpecimen } from "../types";

const THEMES = ["github-dark", "dracula", "catppuccin-mocha", "one-dark", "nord", "github-light", "solarized-light", "pip-boy"];
mockCommand("list_themes", () => THEMES);
mockCommand("get_themes_dir", () => "/Users/andrey/.codeterm/themes");
mockCommand("get_autostart_enabled", () => true);
mockCommand("shell_profiles", (): ShellProfile[] => [
  { id: "zsh", name: "zsh", executable: "/bin/zsh", args: ["-l"], wsl_distro: null },
  { id: "pwsh", name: "PowerShell 7", executable: "pwsh", args: [], wsl_distro: null },
]);

const CONFIG = appConfig(httpApi("settings"));
const save = async () => undefined;
const close = () => undefined;

function PanelSpecimen({ section }: { section?: SettingsLinkId }) {
  return <div className="gallery-fill gallery-panel">
    <SettingsPanel config={CONFIG} onUpdateConfig={save} onClose={close} initialSection={section} />
  </div>;
}

export default defineSpecimen({
  id: "settings-panel",
  title: "Settings panel",
  category: "Settings",
  where: "src/components/Settings/SettingsPanel.tsx",
  stage: "full",
  notes: "The shipped Settings dialog: one Back pill instead of the X, the root section grid on ct-card nav items, and section pages of top-aligned compact settings cards. Check for updates is left out: UpdateSection needs the app-level UpdateProvider and its live update daemon. Themes, autostart and shell profiles come from mocked IPC; daemon HTTP answers from a gallery origin.",
  Component: PanelSpecimen,
  states: [
    { id: "root", label: "Root · section grid · close pill", props: {} },
    { id: "general", label: "General page · settings cards", props: { section: "general" } },
  ],
});
