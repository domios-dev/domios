import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AppConfig, ProviderConfig } from "../../lib/types";
import { MeshSection } from "./MeshSection";
import { CloseXIcon, CheckIcon, CopyIcon } from "../Icons";
import { isMac } from "../../lib/keybindings";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { SchemaRenderer } from "./SchemaRenderer";

interface SettingsPanelProps {
  config: AppConfig;
  onUpdateConfig: (config: AppConfig) => Promise<void>;
  onClose: () => void;
}

enum SettingsTab {
  General = "general",
  Memory = "memory",
  Mesh = "mesh",
  Notifications = "notifications",
  Agents = "agents",
  Integrations = "integrations",
  About = "about",
}

const TAB_ICONS: Record<SettingsTab, string> = {
  [SettingsTab.General]: "M8 16A8 8 0 1 0 8 0a8 8 0 0 0 0 16ZM7 6.5C7 5.672 7.448 5 8 5s1 .672 1 1.5S8.552 8 8 8s-1-.672-1-1.5ZM4.255 9.515a.75.75 0 0 1 .98.058 3.997 3.997 0 0 0 5.53 0 .75.75 0 0 1 1.037 1.085 5.497 5.497 0 0 1-7.607 0 .75.75 0 0 1 .058-.98Z",
  [SettingsTab.Memory]: "M8 0a8 8 0 1 1 0 16A8 8 0 0 1 8 0ZM4.5 7.5a.5.5 0 0 0 0 1h2.793l-2.147 2.146a.5.5 0 0 0 .708.708L8 9.207l2.146 2.147a.5.5 0 0 0 .708-.708L8.707 8.5H11.5a.5.5 0 0 0 0-1H8.707l2.147-2.146a.5.5 0 0 0-.708-.708L8 6.793 5.854 4.646a.5.5 0 1 0-.708.708L7.293 7.5Z",
  [SettingsTab.Mesh]: "M8 1a1 1 0 0 1 1 1v1.07A5.5 5.5 0 0 1 12.93 7H14a1 1 0 1 1 0 2h-1.07A5.5 5.5 0 0 1 9 12.93V14a1 1 0 1 1-2 0v-1.07A5.5 5.5 0 0 1 3.07 9H2a1 1 0 0 1 0-2h1.07A5.5 5.5 0 0 1 7 3.07V2a1 1 0 0 1 1-1Zm0 4a3 3 0 1 0 0 6 3 3 0 0 0 0-6Z",
  [SettingsTab.Notifications]: "M8 16a2 2 0 0 0 1.985-1.75c.017-.137-.097-.25-.235-.25h-3.5c-.138 0-.252.113-.235.25A2 2 0 0 0 8 16ZM3 5a5 5 0 0 1 10 0c0 1.862.818 2.726 1.49 3.434.233.246.46.485.653.752.19.262.357.571.357.974 0 .593-.477 1.09-1.107 1.168C13.477 11.44 12.025 11.5 8 11.5c-4.025 0-5.477-.06-6.393-.172C.977 11.24.5 10.743.5 10.16c0-.403.167-.712.356-.974a6.96 6.96 0 0 0 .653-.752C2.182 7.726 3 6.862 3 5Z",
  [SettingsTab.Agents]: "M5.75 7.5a.75.75 0 0 1 .75.75v1.5a.75.75 0 0 1-1.5 0v-1.5a.75.75 0 0 1 .75-.75Zm5.25.75a.75.75 0 0 0-1.5 0v1.5a.75.75 0 0 0 1.5 0ZM6 1h4a4 4 0 0 1 4 4v6a4 4 0 0 1-4 4H6a4 4 0 0 1-4-4V5a4 4 0 0 1 4-4Zm-.25 1.5A2.75 2.75 0 0 0 3 5.25v5.5A2.75 2.75 0 0 0 5.75 13.5h4.5a2.75 2.75 0 0 0 2.75-2.75v-5.5A2.75 2.75 0 0 0 10.25 2.5Z",
  [SettingsTab.Integrations]: "M8.75 3.75a.75.75 0 0 0-1.5 0v3.5h-3.5a.75.75 0 0 0 0 1.5h3.5v3.5a.75.75 0 0 0 1.5 0v-3.5h3.5a.75.75 0 0 0 0-1.5h-3.5ZM8 16A8 8 0 1 0 8 0a8 8 0 0 0 0 16Zm0-1.5a6.5 6.5 0 1 1 0-13 6.5 6.5 0 0 1 0 13Z",
  [SettingsTab.About]: "M0 8a8 8 0 1 1 16 0A8 8 0 0 1 0 8Zm8-6.5a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13ZM6.5 7.75A.75.75 0 0 1 7.25 7h1a.75.75 0 0 1 .75.75v2.75h.25a.75.75 0 0 1 0 1.5h-2a.75.75 0 0 1 0-1.5h.25v-2h-.25a.75.75 0 0 1-.75-.75ZM8 6a1 1 0 1 1 0-2 1 1 0 0 1 0 2Z",
};

const TABS: { id: SettingsTab; label: string }[] = [
  { id: SettingsTab.General, label: "General" },
  { id: SettingsTab.Memory, label: "Memory" },
  { id: SettingsTab.Mesh, label: "Mesh" },
  { id: SettingsTab.Notifications, label: "Notifications" },
  { id: SettingsTab.Agents, label: "Agents" },
  { id: SettingsTab.Integrations, label: "Integrations" },
  { id: SettingsTab.About, label: "About" },
];

export function SettingsPanel({
  config,
  onUpdateConfig,
  onClose,
}: SettingsPanelProps) {
  const [activeTab, setActiveTab] = useState(SettingsTab.General);
  const [localConfig, setLocalConfig] = useState<AppConfig>(config);
  const [themes, setThemes] = useState<string[]>([]);
  const [configPath, setConfigPath] = useState<string>("");
  const [themesDir, setThemesDir] = useState<string>("");

  const builtinThemes = ["github-dark", "dracula", "catppuccin-mocha", "one-dark", "nord", "github-light", "solarized-light", "pip-boy"];

  const refreshThemes = useCallback(() => {
    invoke<string[]>("list_themes").then(setThemes).catch(() => setThemes(builtinThemes));
  }, []);

  useEffect(() => {
    setLocalConfig(config);
  }, [config]);

  useEffect(() => {
    refreshThemes();
    invoke<string>("get_config_path").then(setConfigPath).catch(() => {});
    invoke<string>("get_themes_dir").then(setThemesDir).catch(() => {});
  }, []);

  const update = useCallback(
    <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => {
      setLocalConfig((prev) => {
        const next = { ...prev, [section]: Object.assign({}, prev[section], patch) };
        onUpdateConfig(next).catch(() => {});
        return next;
      });
    },
    [onUpdateConfig],
  );

  return (
    <div className="absolute inset-0 flex flex-col bg-[var(--ct-sidebar-bg)]">
      <div className="absolute inset-0 flex flex-col overflow-hidden relative">
        {/* Subtle dot grid background */}
        <div
          className="absolute inset-0 pointer-events-none"
          style={{
            backgroundImage: "radial-gradient(var(--ct-border) 1px, transparent 1px)",
            backgroundSize: "20px 20px",
            opacity: 0.3,
          }}
        />
        {/* Header */}
        <div className="flex items-center justify-between px-5 py-3 border-b border-[var(--ct-border)] relative z-10">
          <h2 className="text-sm font-medium text-[var(--ct-fg)]">Settings</h2>
          <button
            onClick={onClose}
            className="flex items-center justify-center w-7 h-7 rounded-md hover:bg-[var(--ct-hover)] text-[var(--ct-muted)] hover:text-[var(--ct-fg)] transition-colors"
          >
            <CloseXIcon />
          </button>
        </div>

        {/* Body: sidebar tabs + content */}
        <div className="flex flex-1 min-h-0 relative z-10">
          {/* Tab sidebar */}
          <div className="w-[170px] shrink-0 border-r border-[var(--ct-border)] py-3 px-2 flex flex-col gap-0.5">
            {TABS.map((tab) => (
              <button
                key={tab.id}
                onClick={() => setActiveTab(tab.id)}
                className={`flex items-center gap-2.5 px-3 py-1.5 text-[11px] rounded-md transition-colors ${
                  activeTab === tab.id
                    ? "bg-[var(--ct-active)] text-[var(--ct-fg)]"
                    : "text-[var(--ct-muted)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"
                }`}
              >
                <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor" className={`shrink-0 ${activeTab === tab.id ? "opacity-90" : "opacity-50"}`}>
                  <path d={TAB_ICONS[tab.id]} />
                </svg>
                <span>{tab.label}</span>
              </button>
            ))}
          </div>

          {/* Content area */}
          <div className="flex-1 overflow-y-auto px-5 py-4">
            {activeTab === SettingsTab.General && (
              <div className="space-y-6">
                <Group title="Color Theme">
                  <Label text="Theme">
                    <select
                      value={localConfig.theme.name}
                      onChange={(e) => update("theme", { name: e.target.value })}
                      className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
                    >
                      <optgroup label="Built-in">
                        {themes.filter(t => builtinThemes.includes(t)).map((t) => (
                          <option key={t} value={t}>{t}</option>
                        ))}
                      </optgroup>
                      {themes.some(t => !builtinThemes.includes(t)) && (
                        <optgroup label="Custom">
                          {themes.filter(t => !builtinThemes.includes(t)).map((t) => (
                            <option key={t} value={t}>{t}</option>
                          ))}
                        </optgroup>
                      )}
                    </select>
                  </Label>
                  <div className="flex items-center gap-2 mt-1">
                    <div className="flex gap-1 flex-1">
                      {[
                        localConfig.theme.background,
                        localConfig.theme.foreground,
                        localConfig.theme.red,
                        localConfig.theme.green,
                        localConfig.theme.yellow,
                        localConfig.theme.blue,
                        localConfig.theme.magenta,
                        localConfig.theme.cyan,
                      ].map((color, i) => (
                        <div
                          key={i}
                          className="w-5 h-5 rounded border border-[var(--ct-border)]"
                          style={{ backgroundColor: color }}
                          title={color}
                        />
                      ))}
                    </div>
                    {themesDir && (
                      <button
                        onClick={() => invoke("open_folder", { path: themesDir })}
                        className="text-[10px] text-[var(--ct-accent)] hover:underline shrink-0"
                        title={`Open ${themesDir}`}
                      >
                        Open themes folder
                      </button>
                    )}
                  </div>
                  {!builtinThemes.includes(localConfig.theme.name) && (
                    <div className="flex items-center justify-between mt-1">
                      <span className="text-[10px] text-[var(--ct-muted-fg)]">Custom theme</span>
                      <button
                        onClick={async () => {
                          await invoke("delete_custom_theme", { themeId: localConfig.theme.name });
                          update("theme", { name: "github-dark" });
                          refreshThemes();
                        }}
                        className="text-[10px] text-[var(--ct-red)] hover:underline"
                      >
                        Delete
                      </button>
                    </div>
                  )}
                </Group>

                <Group title="Font">
                  <Label text="Font family">
                    <input
                      type="text"
                      value={localConfig.font.family}
                      onChange={(e) => update("font", { family: e.target.value })}
                      className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
                    />
                  </Label>
                  <Label text={`Size: ${localConfig.font.size}px`}>
                    <input
                      type="range"
                      min={8}
                      max={28}
                      value={localConfig.font.size}
                      onChange={(e) => update("font", { size: parseInt(e.target.value, 10) })}
                      className="w-full accent-[var(--ct-accent)]"
                    />
                  </Label>
                  <Toggle
                    label="Ligatures"
                    checked={localConfig.font.ligatures}
                    onChange={(v) => update("font", { ligatures: v })}
                  />
                </Group>

                <Group title="Cursor">
                  <Label text="Style">
                    <select
                      value={localConfig.terminal.cursor_style}
                      onChange={(e) => update("terminal", { cursor_style: e.target.value })}
                      className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
                    >
                      <option value="block">Block</option>
                      <option value="underline">Underline</option>
                      <option value="bar">Bar</option>
                    </select>
                  </Label>
                  <Toggle
                    label="Blink"
                    checked={localConfig.terminal.cursor_blink}
                    onChange={(v) => update("terminal", { cursor_blink: v })}
                  />
                </Group>

                <Group title="Scrollback">
                  <Label text={`Buffer: ${localConfig.terminal.scrollback.toLocaleString()} lines`}>
                    <input
                      type="range"
                      min={1000}
                      max={100000}
                      step={1000}
                      value={localConfig.terminal.scrollback}
                      onChange={(e) => update("terminal", { scrollback: parseInt(e.target.value, 10) })}
                      className="w-full accent-[var(--ct-accent)]"
                    />
                  </Label>
                </Group>

                <Group title="Behavior">
                  <Toggle
                    label="Copy on select"
                    checked={localConfig.terminal.copy_on_select}
                    onChange={(v) => update("terminal", { copy_on_select: v })}
                  />
                </Group>

                <KeybindGroup title="Tabs" bindings={[
                  ["new_tab", "New Tab"],
                  ["close_tab", "Close Tab"],
                ]} config={localConfig} onUpdate={(c) => { setLocalConfig(c); onUpdateConfig(c).catch(() => {}); }} />

                <KeybindGroup title="Panes" bindings={[
                  ["close_pane", "Close Pane"],
                  ["focus_pane_left", "Focus Left"],
                  ["focus_pane_right", "Focus Right"],
                  ["focus_pane_up", "Focus Up"],
                  ["focus_pane_down", "Focus Down"],
                ]} config={localConfig} onUpdate={(c) => { setLocalConfig(c); onUpdateConfig(c).catch(() => {}); }} />

                <KeybindGroup title="General" bindings={[
                  ["new_workspace", "New Workspace"],
                  ["search", "Search"],
                  ["zoom_in", "Zoom In"],
                  ["zoom_out", "Zoom Out"],
                  ["zoom_reset", "Zoom Reset"],
                  ["open_settings", "Settings"],
                ]} config={localConfig} onUpdate={(c) => { setLocalConfig(c); onUpdateConfig(c).catch(() => {}); }} />
              </div>
            )}

            {activeTab === SettingsTab.Notifications && (
              <div className="space-y-6">
                <Group title="System Notifications">
                  <div className="text-xs text-[var(--ct-muted-fg)] mb-1">
                    Native OS notifications when CodeTerm window is not focused.
                  </div>
                  <Toggle
                    label="Desktop notifications"
                    checked={localConfig.notifications.desktop}
                    onChange={(v) => update("notifications", { desktop: v })}
                  />
                  <Toggle
                    label="Sound"
                    checked={localConfig.notifications.sound}
                    onChange={(v) => update("notifications", { sound: v })}
                  />
                </Group>

                <Group title="In-app Toast">
                  <div className="text-xs text-[var(--ct-muted-fg)] mb-1">
                    Toast notifications appear inside CodeTerm for 5 seconds.
                  </div>
                  <Toggle
                    label="Show toast notifications"
                    checked={true}
                    onChange={() => {}}
                  />
                </Group>

                <ProviderNotifications
                  fullConfig={localConfig}
                  onUpdateFullConfig={(next) => {
                    setLocalConfig(next);
                    onUpdateConfig(next).catch(() => {});
                  }}
                />
              </div>
            )}

            {activeTab === SettingsTab.Memory && (
              <MemoryTab config={localConfig} onUpdate={(section, patch) => update(section, patch)} />
            )}

            {activeTab === SettingsTab.Agents && (
              <AgentsTab
                config={localConfig}
                onUpdate={(section, patch) => update(section, patch)}
                fullConfig={localConfig}
                onUpdateFullConfig={(next) => {
                  setLocalConfig(next);
                  onUpdateConfig(next).catch(() => {});
                }}
              />
            )}

            {activeTab === SettingsTab.Integrations && (
              <IntegrationsTab config={localConfig} onUpdate={(section, patch) => update(section, patch)} />
            )}

            {activeTab === SettingsTab.Mesh && (
              <NetworkTab
                config={localConfig}
                onUpdate={(section, patch) => update(section, patch)}
              />
            )}


            {activeTab === SettingsTab.About && (
              <div className="space-y-6">
                {/* Logo + tagline */}
                <div className="text-center py-4">
                  <img src="/assets/logo.png" alt="CodeTerm" width="64" height="64" className="rounded-xl mb-3 mx-auto" />
                  <div className="text-xl font-bold text-[var(--ct-fg)]">CodeTerm</div>
                  <div className="text-xs text-[var(--ct-muted-fg)] mt-1">AI-first terminal multiplexer</div>
                </div>

                <Group title="Info">
                  <InfoRow label="Version" value={__APP_VERSION__} />
                  <InfoRow label="Platform" value={navigator.platform} />
                  <InfoRow label="Config" value={configPath || "loading..."} mono />
                  <CheckForUpdateButton />
                </Group>

                {/* Creator */}
                <Group title="Created by">
                  <div className="text-sm text-[var(--ct-fg)]">Andrey Kovalev</div>
                  <div className="flex flex-wrap gap-3 mt-2">
                    <AboutLink href="https://github.com/rollacode" label="GitHub" icon="M8 0c4.42 0 8 3.58 8 8a8.01 8.01 0 0 1-5.45 7.59c-.4.08-.55-.17-.55-.38 0-.27.01-1.13.01-2.2 0-.75-.25-1.23-.54-1.48 1.78-.2 3.65-.88 3.65-3.95 0-.88-.31-1.59-.82-2.15.08-.2.36-1.02-.08-2.12 0 0-.67-.22-2.2.82-.64-.18-1.32-.27-2-.27-.68 0-1.36.09-2 .27-1.53-1.03-2.2-.82-2.2-.82-.44 1.1-.16 1.92-.08 2.12-.51.56-.82 1.28-.82 2.15 0 3.06 1.86 3.75 3.64 3.95-.23.2-.44.55-.51 1.07-.46.21-1.61.55-2.33-.66-.15-.24-.6-.83-1.23-.82-.67.01-.27.38.01.53.34.19.73.9.82 1.13.16.45.68 1.31 2.69.94 0 .67.01 1.3.01 1.49 0 .21-.15.45-.55.38A7.995 7.995 0 0 1 0 8c0-4.42 3.58-8 8-8Z" />
                    <AboutLink href="https://rollacode.org" label="Website" icon="M0 8a8 8 0 1 1 16 0A8 8 0 0 1 0 8Zm8-6.5a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13ZM6.5 7.75A.75.75 0 0 1 7.25 7h1a.75.75 0 0 1 .75.75v2.75h.25a.75.75 0 0 1 0 1.5h-2a.75.75 0 0 1 0-1.5h.25v-2h-.25a.75.75 0 0 1-.75-.75ZM8 6a1 1 0 1 1 0-2 1 1 0 0 1 0 2Z" />
                    <AboutLink href="https://www.linkedin.com/in/rollacode" label="LinkedIn" icon="M12.225 12.225h-1.778V9.44c0-.664-.012-1.519-.925-1.519-.926 0-1.068.724-1.068 1.47v2.834H6.676V6.498h1.707v.783h.024c.348-.594.996-.95 1.684-.925 1.802 0 2.135 1.185 2.135 2.728ZM3.56 5.715a1.03 1.03 0 1 1 0-2.06 1.03 1.03 0 0 1 0 2.06Zm.889 6.51H2.67V6.498h1.779ZM13.11 2H2.885A.88.88 0 0 0 2 2.866v10.268a.88.88 0 0 0 .885.866h10.226a.882.882 0 0 0 .889-.866V2.865A.88.88 0 0 0 13.111 2Z" />
                  </div>
                </Group>

                <Group title="Setup">
                  <button
                    onClick={() => {
                      const next = { ...localConfig, onboarding_completed: false };
                      onUpdateConfig(next).then(() => {
                        setLocalConfig(next);
                        window.location.reload();
                      }).catch(() => {});
                    }}
                    className="px-3 py-1.5 text-[11px] rounded bg-[var(--ct-border)] text-[var(--ct-fg)] hover:bg-[var(--ct-muted)] transition-colors"
                  >
                    Reset Onboarding
                  </button>
                  <div className="text-[10px] text-[var(--ct-muted)]">
                    Re-run the setup wizard on next launch.
                  </div>
                </Group>

                <div className="text-center pt-2">
                  <span className="text-xs text-[var(--ct-muted)]">Let&apos;s build something great :)</span>
                </div>
              </div>
            )}
          </div>
        </div>

      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Sub-components
// ---------------------------------------------------------------------------

function CheckForUpdateButton() {
  const [status, setStatus] = useState<"idle" | "checking" | "up-to-date" | "available" | "installing" | "error">("idle");
  const [info, setInfo] = useState<{ latest_version?: string; release_url?: string } | null>(null);

  const check = async () => {
    setStatus("checking");
    try {
      const result = await invoke<{
        update_available: boolean;
        latest_version: string;
        release_url: string;
      }>("check_for_update");
      if (result.update_available) {
        setStatus("available");
        setInfo(result);
      } else {
        setStatus("up-to-date");
        setTimeout(() => setStatus("idle"), 3000);
      }
    } catch {
      setStatus("error");
      setTimeout(() => setStatus("idle"), 3000);
    }
  };

  const install = async () => {
    setStatus("installing");
    try {
      await invoke("install_app_update");
    } catch {
      setStatus("error");
      setTimeout(() => setStatus("available"), 3000);
    }
  };

  return (
    <div className="flex items-center gap-3 flex-wrap">
      <button
        onClick={check}
        disabled={status === "checking" || status === "installing"}
        className="px-3 py-1.5 text-[11px] rounded bg-[var(--ct-border)] text-[var(--ct-fg)] hover:bg-[var(--ct-muted)] transition-colors disabled:opacity-50"
      >
        {status === "checking" ? "Checking..." : "Check for Updates"}
      </button>
      {status === "up-to-date" && (
        <span className="text-[11px] text-[var(--ct-green)]">Up to date</span>
      )}
      {status === "available" && info && (
        <>
          <span className="text-[11px] text-[var(--ct-yellow)]">v{info.latest_version} available</span>
          <button
            onClick={install}
            className="px-3 py-1.5 text-[11px] rounded bg-[var(--ct-green)] text-[var(--ct-bg)] hover:opacity-90 transition-opacity font-medium"
          >
            Install & Restart
          </button>
        </>
      )}
      {status === "installing" && (
        <span className="text-[11px] text-[var(--ct-muted-fg)]">Downloading and installing...</span>
      )}
      {status === "error" && (
        <span className="text-[11px] text-[var(--ct-red)]">Update failed</span>
      )}
    </div>
  );
}

export function Group({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div>
      <h3 className="text-[10px] font-medium uppercase tracking-[0.06em] text-[var(--ct-muted)] mb-2.5">{title}</h3>
      <div className="space-y-2.5">{children}</div>
    </div>
  );
}

function Label({ text, children }: { text: string; children: React.ReactNode }) {
  return (
    <label className="block">
      <span className="block text-xs text-[var(--ct-muted-fg)] mb-1">{text}</span>
      {children}
    </label>
  );
}

export function Toggle({ label, checked, onChange }: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="flex items-center justify-between cursor-pointer">
      <span className="text-xs text-[var(--ct-muted-fg)]">{label}</span>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        onClick={() => onChange(!checked)}
        className={`relative inline-flex h-5 w-9 items-center rounded-full transition-colors ${
          checked ? "bg-[var(--ct-accent)]" : "bg-[var(--ct-border)]"
        }`}
      >
        <span
          className={`inline-block h-3.5 w-3.5 rounded-full bg-white transition-transform ${
            checked ? "translate-x-4" : "translate-x-0.5"
          }`}
        />
      </button>
    </label>
  );
}

export function CopyRow({ label, value }: { label: string; value: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <div className="flex items-center justify-between text-xs gap-2">
      <span className="text-[var(--ct-muted)] shrink-0">{label}</span>
      <div className="flex items-center gap-1 min-w-0">
        <code className="bg-[var(--ct-bg)] px-1.5 py-0.5 rounded text-[var(--ct-fg)] text-[10px] truncate">{value}</code>
        <button
          onClick={() => {
            navigator.clipboard.writeText(value).then(() => {
              setCopied(true);
              setTimeout(() => setCopied(false), 1500);
            });
          }}
          className="shrink-0 w-5 h-5 flex items-center justify-center rounded hover:bg-[var(--ct-border)] text-[var(--ct-muted)] hover:text-[var(--ct-fg)] transition-colors"
          title="Copy"
        >
          {copied ? (
            <CheckIcon />
          ) : (
            <CopyIcon />
          )}
        </button>
      </div>
    </div>
  );
}

export function InfoRow({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="flex items-center justify-between text-xs">
      <span className="text-[var(--ct-muted)]">{label}</span>
      {mono ? (
        <code className="bg-[var(--ct-bg)] px-1.5 py-0.5 rounded text-[var(--ct-fg)] text-[10px] max-w-[280px] truncate">
          {value}
        </code>
      ) : (
        <span className="text-[var(--ct-fg)]">{value}</span>
      )}
    </div>
  );
}

function AboutLink({ href, label, icon }: { href: string; label: string; icon: string }) {
  return (
    <a
      href={href}
      target="_blank"
      rel="noopener noreferrer"
      className="inline-flex items-center gap-1.5 text-xs text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] transition-colors"
    >
      <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor" className="shrink-0">
        <path d={icon} />
      </svg>
      <span>{label}</span>
    </a>
  );
}


const KEY_SYMBOLS: Record<string, string> = isMac ? {
  ctrl: "⌃",
  shift: "⇧",
  alt: "⌥",
  cmd: "⌘",
  mod: "⌘",
  super: "⌘",
  opt: "⌥",
  option: "⌥",
  tab: "⇥",
  enter: "↩",
  backspace: "⌫",
  delete: "⌦",
  escape: "⎋",
  arrowleft: "←",
  arrowright: "→",
  arrowup: "↑",
  arrowdown: "↓",
  " ": "Space",
} : {
  ctrl: "Ctrl",
  shift: "Shift",
  alt: "Alt",
  cmd: "Ctrl",
  mod: "Ctrl",
  super: "Win",
  tab: "Tab",
  enter: "Enter",
  backspace: "Bksp",
  delete: "Del",
  escape: "Esc",
  arrowleft: "←",
  arrowright: "→",
  arrowup: "↑",
  arrowdown: "↓",
  " ": "Space",
};

function renderKbd(binding: string) {
  const parts = binding.split("+").map((p) => p.trim());
  return (
    <span className="flex items-center gap-0.5">
      {parts.map((part, i) => {
        const sym = KEY_SYMBOLS[part.toLowerCase()] || part.toUpperCase();
        // On mac, Ctrl in binding with letters = Mod = ⌘
        const display = isMac && part.toLowerCase() === "ctrl" && parts.some(p => /^[a-zA-Z0-9]$/.test(p))
          ? "⌘" : sym;
        return (
          <kbd
            key={i}
            className="inline-flex items-center justify-center min-w-[20px] h-[20px] px-1 rounded bg-[var(--ct-bg)] border border-[var(--ct-border)] text-[10px] font-mono text-[var(--ct-muted-fg)] shadow-[0_1px_0_var(--ct-border)]"
          >
            {display}
          </kbd>
        );
      })}
    </span>
  );
}

function KeybindGroup({
  title,
  bindings,
  config,
  onUpdate,
}: {
  title: string;
  bindings: [string, string][];
  config: AppConfig;
  onUpdate: (config: AppConfig) => void;
}) {
  const [editingKey, setEditingKey] = useState<string | null>(null);

  return (
    <Group title={title}>
      <div className="space-y-1">
        {bindings.map(([key, label]) => {
          const value = (config.keybindings as unknown as Record<string, string>)[key] || "";
          const isEditing = editingKey === key;
          return (
            <div key={key} className="flex items-center justify-between py-0.5">
              <span className="text-xs text-[var(--ct-muted-fg)]">{label}</span>
              {isEditing ? (
                <input
                  type="text"
                  autoFocus
                  value={value}
                  onChange={(e) => {
                    const next = { ...config, keybindings: { ...config.keybindings, [key]: e.target.value } };
                    onUpdate(next);
                  }}
                  onBlur={() => setEditingKey(null)}
                  onKeyDown={(e) => { if (e.key === "Enter" || e.key === "Escape") setEditingKey(null); }}
                  className="w-36 bg-[var(--ct-bg)] border border-[var(--ct-accent)] rounded px-2 py-0.5 text-[11px] text-[var(--ct-fg)] focus:outline-none text-right font-mono"
                />
              ) : (
                <button
                  onClick={() => setEditingKey(key)}
                  className="flex items-center gap-1 px-1.5 py-0.5 rounded hover:bg-[var(--ct-hover)] transition-colors"
                  title="Click to edit"
                >
                  {renderKbd(value)}
                </button>
              )}
            </div>
          );
        })}
      </div>
    </Group>
  );
}

function NetworkTab({
  config: cfg,
  onUpdate,
}: {
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
}) {
  const [net, setNet] = useState<{
    local_ip: string; tailscale_ip: string | null;
    api_port: number; api_url: string; tailscale_url: string | null;
  } | null>(null);
  useEffect(() => {
    invoke<typeof net>("get_network_info").then(setNet).catch(() => {});
  }, []);

  const apiEnabled = cfg.api?.enabled ?? false;

  return (
    <div className="space-y-6">
      <MeshSection config={cfg} onUpdate={onUpdate} apiEnabled={apiEnabled} />

      <Collapsible title="API Endpoints Reference">
        <div className="space-y-3">
          <EndpointGroup title="Workspaces" endpoints={[
            ["GET", "/api/workspaces", "List all"],
            ["POST", "/api/workspaces", "Create {name}"],
            ["POST", "/api/workspaces/:id/focus", "Switch"],
            ["POST", "/api/workspaces/:id/rename", "Rename {name}"],
            ["POST", "/api/workspaces/:id/close", "Delete"],
            ["POST", "/api/workspaces/:id/tabs", "Create tab"],
            ["POST", "/api/workspaces/:id/tabs/:tid/move", "Move tab"],
          ]} />
          <EndpointGroup title="Panes" endpoints={[
            ["GET", "/api/panes", "List all"],
            ["POST", "/api/panes/:id/focus", "Focus"],
            ["POST", "/api/panes/:id/close", "Close"],
            ["POST", "/api/panes/:id/send", "Send text (fire & forget)"],
            ["POST", "/api/panes/:id/exec", "Send + wait {command, timeout_ms}"],
            ["POST", "/api/panes/:id/rename", "Rename {title}"],
            ["POST", "/api/panes/:id/resize", "Resize {cols, rows}"],
            ["GET", "/api/panes/:id/output", "Get output {?offset=N}"],
            ["GET", "/api/panes/:id/output/stream", "SSE real-time stream"],
          ]} />
          <EndpointGroup title="Integration" endpoints={[
            ["POST", "/api/browser/open", "Open URL {url}"],
            ["POST", "/api/notifications", "Create {title, body, level}"],
            ["GET", "/api/network", "Connection info + IPs"],
            ["GET", "/api/serve-file", "Serve local file {?path=}"],
          ]} />
        </div>
      </Collapsible>
    </div>
  );
}

function AgentsTab({
  config,
  onUpdate,
  fullConfig,
  onUpdateFullConfig,
}: {
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
  fullConfig: AppConfig;
  onUpdateFullConfig: (next: AppConfig) => void;
}) {
  const { providers, loading, error } = useAgentProviders();
  const [activeProviderId, setActiveProviderId] = useState<string | null>(null);

  // Lock the active tab once providers load. Don't reset on subsequent
  // refetches — that would jump the user back to tab 0 on plugin events.
  useEffect(() => {
    if (!activeProviderId && providers.length > 0) {
      setActiveProviderId(providers[0].id);
    }
  }, [providers, activeProviderId]);

  const activeProvider = providers.find((p) => p.id === activeProviderId) ?? null;

  // Per-provider config writer: merges into `agents.<id>` and emits
  // through the parent's update channel.
  const onProviderConfigChange = useCallback(
    (providerId: string, patch: Partial<ProviderConfig>) => {
      const prev = fullConfig.agents?.[providerId] ?? {};
      const nextProvider: ProviderConfig = { ...prev, ...patch };
      const nextAgents = { ...(fullConfig.agents ?? {}), [providerId]: nextProvider };
      onUpdateFullConfig({ ...fullConfig, agents: nextAgents });
    },
    [fullConfig, onUpdateFullConfig],
  );

  return (
    <div className="space-y-0">
      {/* General Agent — shared across all providers */}
      <div className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4 space-y-3 mb-5">
        <div className="text-sm font-medium text-[var(--ct-fg)]">General Agent</div>
        <div className="text-xs text-[var(--ct-muted-fg)]">
          Always-on AI assistant in a protected workspace. Starts automatically with CodeTerm.
        </div>
        <Toggle
          label="Enable General Agent"
          checked={config.general_agent?.enabled ?? true}
          onChange={(v) => onUpdate("general_agent", { enabled: v })}
        />
        {config.general_agent?.enabled && (
          <>
            <div className="flex items-center justify-between">
              <span className="text-xs text-[var(--ct-fg)]">Provider</span>
              <div className="flex items-center gap-2">
                <select
                  value={config.general_agent?.agent_provider ?? "claude"}
                  onChange={(e) => onUpdate("general_agent", { agent_provider: e.target.value })}
                  className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-2 py-1 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
                >
                  {providers.map((p) => (
                    <option key={p.id} value={p.id}>{p.display_name}</option>
                  ))}
                </select>
                <button
                  onClick={() => {
                    invoke("restart_general_agent").catch((e) => console.error("GA restart failed:", e));
                  }}
                  className="px-2.5 py-1 text-[10px] font-medium rounded border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] transition-colors"
                >
                  Restart
                </button>
              </div>
            </div>
            <Toggle
              label="Auto-restart on crash"
              checked={config.general_agent?.auto_restart ?? true}
              onChange={(v) => onUpdate("general_agent", { auto_restart: v })}
            />
          </>
        )}
        <div className="text-[10px] text-[var(--ct-muted)]">
          Changes provider on next restart, or click Restart now.
        </div>
      </div>

      {/* Skills — CodeTerm-wide source of truth, NOT a provider contribution.
          Hardcoded here on purpose: skills are a CodeTerm feature that
          syncs to all providers, not something Claude or Codex own. The
          Phase 3 broker will replace the auto-mirror with on-demand CLI
          access (see docs/plans/memory-architecture.md). */}
      <div className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4 space-y-3 mb-5">
        <div className="flex items-center justify-between">
          <div className="text-sm font-medium text-[var(--ct-fg)]">Skills</div>
          <span className="text-[9px] px-1.5 py-0.5 rounded-full bg-[var(--ct-green)]/20 text-[var(--ct-green)]">
            auto-synced
          </span>
        </div>
        <div className="text-xs text-[var(--ct-muted-fg)]">
          Drop skill folders into <code className="text-[10px]">~/.codeterm/skills/</code> — they're
          available to every agent provider automatically. Source of truth lives in CodeTerm; providers
          read on demand. The dedicated <code className="text-[10px]">codeterm skill</code> CLI broker
          arrives with the memory plugin (Phase 3).
        </div>
        <div className="flex items-center justify-between text-[10px] text-[var(--ct-muted)]">
          <span className="font-mono">~/.codeterm/skills/</span>
          <button
            onClick={() => invoke("open_folder", { path: "~/.codeterm/skills" }).catch(() => {})}
            className="text-[var(--ct-accent)] hover:underline"
          >
            Open folder
          </button>
        </div>
      </div>

      {error && (
        <div className="text-[10px] text-[var(--ct-red)] mb-3">
          Failed to load agent providers: {error}
        </div>
      )}
      {loading && providers.length === 0 && (
        <div className="text-[10px] text-[var(--ct-muted-fg)]">Loading providers…</div>
      )}

      {/* Provider sub-tabs (data-driven, no hardcode) */}
      {providers.length > 0 && (
        <div className="flex gap-[2px] -mx-5 px-2 pt-2 pb-0 border-b border-[var(--ct-border)] bg-[var(--ct-sidebar-bg)]">
          {providers.map((p) => (
            <button
              key={p.id}
              onClick={() => setActiveProviderId(p.id)}
              className={`flex items-center gap-2 px-4 py-2.5 text-[13px] font-medium rounded-t-lg border border-transparent border-b-0 relative transition-colors select-none ${
                activeProviderId === p.id
                  ? "text-[var(--ct-fg)] bg-[var(--ct-sidebar-bg)] border-[var(--ct-border)] -mb-px z-[1]"
                  : "text-[var(--ct-muted)] hover:text-[var(--ct-muted-fg)] hover:bg-[var(--ct-bg)]/50"
              }`}
            >
              {p.icon_hint && <span className="text-sm leading-none">{p.icon_hint}</span>}
              {p.display_name}
              <span className={`w-1.5 h-1.5 rounded-full bg-[var(--ct-green)] transition-opacity ${
                activeProviderId === p.id ? "opacity-100" : "opacity-0"
              }`} />
            </button>
          ))}
        </div>
      )}

      {/* Provider content — fully data-driven from settings_schema */}
      {activeProvider && (
        <div className="pt-5">
          <SchemaRenderer
            schema={activeProvider.settings_schema}
            providerId={activeProvider.id}
            config={fullConfig}
            onChange={onProviderConfigChange}
          />
        </div>
      )}
    </div>
  );
}

function MemoryTab({ config, onUpdate }: { config: AppConfig; onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void }) {
  const defaultMem = { extraction_agent: "", extraction_model: "haiku", auto_extract: true, dreaming_enabled: true, dreaming_agent: "", dreaming_model: "haiku" };
  const mem = { ...defaultMem, ...(config.memory ?? {}) };
  const { providers } = useAgentProviders();
  const inputClass = "w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none";

  const defaultProvider = config.general_agent?.agent_provider || "claude";

  // Extraction models
  const extractionProvider = mem.extraction_agent || defaultProvider;
  const extractionModels = providers.find(p => p.id === extractionProvider)?.available_models ?? [];

  // Dreaming models
  const dreamingProvider = mem.dreaming_agent || defaultProvider;
  const dreamingModels = providers.find(p => p.id === dreamingProvider)?.available_models ?? [];

  return (
    <div className="space-y-6">
      <Group title="Extraction Workers">
        <p className="text-[11px] text-[var(--ct-muted)] mb-3">
          When you save memories, extraction workers analyze the content and build a knowledge graph — entities, relations, decisions, problems. Workers run as headless agents in the Memory Panel.
        </p>
        <Label text="Auto-extraction">
          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={mem.auto_extract}
              onChange={(e) => onUpdate("memory", { ...mem, auto_extract: e.target.checked })}
              className="w-4 h-4 rounded"
            />
            <span className="text-xs text-[var(--ct-fg)]">Spawn extraction workers on save</span>
          </label>
        </Label>
        <Label text="Agent provider">
          <select
            value={mem.extraction_agent || ""}
            onChange={(e) => onUpdate("memory", { ...mem, extraction_agent: e.target.value })}
            className={inputClass}
          >
            <option value="">Default ({config.general_agent?.agent_provider || "claude"})</option>
            {providers.map(p => (
              <option key={p.id} value={p.id}>{p.display_name}</option>
            ))}
          </select>
          <p className="text-[10px] text-[var(--ct-muted)] mt-1">Which AI agent runs extraction workers.</p>
        </Label>
        <Label text="Model">
          {extractionModels.length > 0 ? (
            <select
              value={mem.extraction_model}
              onChange={(e) => onUpdate("memory", { ...mem, extraction_model: e.target.value })}
              className={inputClass}
            >
              <option value="">Provider default</option>
              {extractionModels.map(m => (
                <option key={m.id} value={m.id}>{m.display_name}</option>
              ))}
            </select>
          ) : (
            <input
              value={mem.extraction_model}
              onChange={(e) => onUpdate("memory", { ...mem, extraction_model: e.target.value })}
              className={inputClass}
              placeholder="haiku, sonnet, o3-mini..."
            />
          )}
          <p className="text-[10px] text-[var(--ct-muted)] mt-1">
            Smaller models (haiku, o3-mini) are faster and cheaper for extraction. On Claude Max, model may be overridden by subscription.
          </p>
        </Label>
      </Group>

      <Group title="Nightly Optimizer (Dreaming)">
        <p className="text-[11px] text-[var(--ct-muted)] mb-3">
          Runs at ~3 AM (±90 min jitter for anti-detection). Promotes validated learnings to facts, removes stale conversations, deduplicates items, classifies areas, cleans orphan nodes. Cannot be deleted, only toggled.
        </p>
        <Label text="Dreaming">
          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={mem.dreaming_enabled}
              onChange={(e) => onUpdate("memory", { ...mem, dreaming_enabled: e.target.checked })}
              className="w-4 h-4 rounded"
            />
            <span className="text-xs text-[var(--ct-fg)]">Nightly memory optimization</span>
          </label>
        </Label>
        <Label text="Agent provider">
          <select
            value={mem.dreaming_agent || ""}
            onChange={(e) => onUpdate("memory", { ...mem, dreaming_agent: e.target.value })}
            className={inputClass}
          >
            <option value="">Default ({defaultProvider})</option>
            {providers.map(p => (
              <option key={p.id} value={p.id}>{p.display_name}</option>
            ))}
          </select>
        </Label>
        <Label text="Model">
          {dreamingModels.length > 0 ? (
            <select
              value={mem.dreaming_model}
              onChange={(e) => onUpdate("memory", { ...mem, dreaming_model: e.target.value })}
              className={inputClass}
            >
              <option value="">Provider default</option>
              {dreamingModels.map(m => (
                <option key={m.id} value={m.id}>{m.display_name}</option>
              ))}
            </select>
          ) : (
            <input
              value={mem.dreaming_model}
              onChange={(e) => onUpdate("memory", { ...mem, dreaming_model: e.target.value })}
              className={inputClass}
              placeholder="haiku, sonnet, o3-mini..."
            />
          )}
        </Label>
      </Group>
    </div>
  );
}

function IntegrationsTab({ config, onUpdate }: { config: AppConfig; onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void }) {
  const [allowedUsersText, setAllowedUsersText] = useState(
    (config.integrations?.telegram_allowed_users ?? []).join(", ")
  );

  return (
    <div className="space-y-5">
      <div className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4 space-y-3">
        <div className="flex items-center justify-between">
          <div className="text-sm font-medium text-[var(--ct-fg)]">Telegram</div>
          {config.integrations?.telegram_enabled && (
            <span className="text-[9px] px-1.5 py-0.5 rounded-full bg-[var(--ct-green)]/20 text-[var(--ct-green)]">enabled</span>
          )}
        </div>
        <div className="text-xs text-[var(--ct-muted-fg)]">
          Send messages to your General Agent via Telegram bot. Responses are captured and sent back.
        </div>
        <Toggle
          label="Enable Telegram bridge"
          checked={config.integrations?.telegram_enabled ?? false}
          onChange={(v) => onUpdate("integrations", { telegram_enabled: v })}
        />
        {config.integrations?.telegram_enabled && (
          <>
            <div className="text-xs text-[var(--ct-muted-fg)]">
              Token: <span className="font-mono text-[var(--ct-fg)]">~/.codeterm/secrets/telegram.token</span>
            </div>
            <div className="space-y-1">
              <div className="text-xs text-[var(--ct-muted-fg)]">Allowed user IDs (comma-separated, empty = all)</div>
              <input
                type="text"
                value={allowedUsersText}
                onChange={(e) => {
                  setAllowedUsersText(e.target.value);
                  const ids = e.target.value.split(",").map((s) => s.trim()).filter(Boolean);
                  onUpdate("integrations", { telegram_allowed_users: ids });
                }}
                placeholder="e.g. 123456789, 987654321"
                className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none font-mono"
              />
            </div>
            <div className="text-[10px] text-[var(--ct-muted)]">
              Requires restart. Create a bot via @BotFather and save the token.
            </div>
          </>
        )}
      </div>

      <div className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4 space-y-3 opacity-50">
        <div className="flex items-center justify-between">
          <div className="text-sm font-medium text-[var(--ct-fg)]">Discord</div>
          <span className="text-[9px] px-1.5 py-0.5 rounded-full bg-[var(--ct-border)] text-[var(--ct-muted-fg)]">coming soon</span>
        </div>
        <div className="text-xs text-[var(--ct-muted-fg)]">
          Connect a Discord bot to interact with your General Agent.
        </div>
      </div>
    </div>
  );
}

/**
 * Render the Notifications section contributed by each active agent provider.
 *
 * Iterates `useAgentProviders()` and renders one `<SchemaRenderer />` per
 * provider whose `notifications_schema` is non-empty. Codex contributes
 * nothing today, so only Claude shows up. When a hypothetical third
 * provider plugin contributes a notification schema, it appears here
 * automatically — zero changes to this component.
 */
function ProviderNotifications({
  fullConfig,
  onUpdateFullConfig,
}: {
  fullConfig: AppConfig;
  onUpdateFullConfig: (next: AppConfig) => void;
}) {
  const { providers } = useAgentProviders();

  const onProviderConfigChange = useCallback(
    (providerId: string, patch: Partial<ProviderConfig>) => {
      const prev = fullConfig.agents?.[providerId] ?? {};
      const nextProvider: ProviderConfig = { ...prev, ...patch };
      const nextAgents = { ...(fullConfig.agents ?? {}), [providerId]: nextProvider };
      onUpdateFullConfig({ ...fullConfig, agents: nextAgents });
    },
    [fullConfig, onUpdateFullConfig],
  );

  const contributors = providers.filter((p) => p.notifications_schema.length > 0);
  if (contributors.length === 0) return null;

  return (
    <>
      {contributors.map((p) => (
        <SchemaRenderer
          key={p.id}
          schema={p.notifications_schema}
          providerId={p.id}
          config={fullConfig}
          onChange={onProviderConfigChange}
        />
      ))}
    </>
  );
}

function Collapsible({ title, children }: { title: string; children: React.ReactNode }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="border border-[var(--ct-border)] rounded-md overflow-hidden">
      <button
        onClick={() => setOpen((v) => !v)}
        className="flex items-center justify-between w-full px-3 py-2 text-xs font-medium text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] transition-colors"
      >
        <span>{title}</span>
        <span className="text-[10px]">{open ? "\u25B4" : "\u25BE"}</span>
      </button>
      {open && (
        <div className="px-3 py-2 border-t border-[var(--ct-border)] bg-[var(--ct-bg)]/50">
          {children}
        </div>
      )}
    </div>
  );
}

function EndpointGroup({ title, endpoints }: { title: string; endpoints: [string, string, string][] }) {
  const methodColors: Record<string, string> = {
    GET: "text-[var(--ct-green)]",
    POST: "text-[var(--ct-accent)]",
    PUT: "text-[var(--ct-yellow)]",
    DELETE: "text-[var(--ct-red)]",
  };
  return (
    <div>
      <div className="text-[10px] font-semibold uppercase tracking-wider text-[var(--ct-muted)] mb-1">{title}</div>
      <div className="space-y-0.5">
        {endpoints.map(([method, path, desc], i) => (
          <div key={i} className="flex items-start gap-2 text-[10px] font-mono leading-relaxed">
            <span className={`shrink-0 w-8 font-bold ${methodColors[method] || "text-[var(--ct-muted-fg)]"}`}>{method}</span>
            <span className="text-[var(--ct-fg)] shrink-0">{path}</span>
            <span className="text-[var(--ct-muted)] truncate">{desc}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
