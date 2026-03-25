import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AppConfig } from "../../lib/types";

interface SettingsPanelProps {
  config: AppConfig;
  onUpdateConfig: (config: AppConfig) => Promise<void>;
  onClose: () => void;
}

enum SettingsTab {
  Appearance = "appearance",
  Terminal = "terminal",
  Keybindings = "keybindings",
  Notifications = "notifications",
  Agents = "agents",
  Network = "network",
  About = "about",
}

const TAB_ICONS: Record<SettingsTab, string> = {
  [SettingsTab.Appearance]: "M1 3.5A2.5 2.5 0 0 1 3.5 1h9A2.5 2.5 0 0 1 15 3.5v9a2.5 2.5 0 0 1-2.5 2.5h-9A2.5 2.5 0 0 1 1 12.5Zm2.5-1a1 1 0 0 0-1 1v5.586l2.293-2.293a1 1 0 0 1 1.414 0L9 10.086l1.293-1.293a1 1 0 0 1 1.414 0L14 11.086V3.5a1 1 0 0 0-1-1Zm7 3.5a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3Z",
  [SettingsTab.Terminal]: "M0 2.75C0 1.784.784 1 1.75 1h12.5c.966 0 1.75.784 1.75 1.75v10.5A1.75 1.75 0 0 1 14.25 15H1.75A1.75 1.75 0 0 1 0 13.25Zm1.75-.25a.25.25 0 0 0-.25.25v10.5c0 .138.112.25.25.25h12.5a.25.25 0 0 0 .25-.25V2.75a.25.25 0 0 0-.25-.25ZM3.5 4.5l3 2.5-3 2.5V4.5ZM8.5 10h3v1.5h-3V10Z",
  [SettingsTab.Keybindings]: "M6.5 1.75a.75.75 0 0 1 .75-.75h1.5a.75.75 0 0 1 .75.75v3a.75.75 0 0 1-.75.75h-1.5a.75.75 0 0 1-.75-.75ZM8 8a.75.75 0 0 1 .75.75v5.5a.75.75 0 0 1-1.5 0v-5.5A.75.75 0 0 1 8 8Zm-3.5 1.5a.75.75 0 0 0-1.5 0v4.75a.75.75 0 0 0 1.5 0Zm-1.5-6a.75.75 0 0 1 .75-.75h1.5a.75.75 0 0 1 0 1.5h-1.5A.75.75 0 0 1 3 3.5Zm9.5 6a.75.75 0 0 0-1.5 0v4.75a.75.75 0 0 0 1.5 0Z",
  [SettingsTab.Notifications]: "M8 16a2 2 0 0 0 1.985-1.75c.017-.137-.097-.25-.235-.25h-3.5c-.138 0-.252.113-.235.25A2 2 0 0 0 8 16ZM3 5a5 5 0 0 1 10 0c0 1.862.818 2.726 1.49 3.434.233.246.46.485.653.752.19.262.357.571.357.974 0 .593-.477 1.09-1.107 1.168C13.477 11.44 12.025 11.5 8 11.5c-4.025 0-5.477-.06-6.393-.172C.977 11.24.5 10.743.5 10.16c0-.403.167-.712.356-.974a6.96 6.96 0 0 0 .653-.752C2.182 7.726 3 6.862 3 5Z",
  [SettingsTab.Agents]: "M5.75 7.5a.75.75 0 0 1 .75.75v1.5a.75.75 0 0 1-1.5 0v-1.5a.75.75 0 0 1 .75-.75Zm5.25.75a.75.75 0 0 0-1.5 0v1.5a.75.75 0 0 0 1.5 0ZM6 1h4a4 4 0 0 1 4 4v6a4 4 0 0 1-4 4H6a4 4 0 0 1-4-4V5a4 4 0 0 1 4-4Zm-.25 1.5A2.75 2.75 0 0 0 3 5.25v5.5A2.75 2.75 0 0 0 5.75 13.5h4.5a2.75 2.75 0 0 0 2.75-2.75v-5.5A2.75 2.75 0 0 0 10.25 2.5Z",
  [SettingsTab.Network]: "M1.5 8a6.5 6.5 0 1 1 13 0 6.5 6.5 0 0 1-13 0ZM8 0a8 8 0 1 0 0 16A8 8 0 0 0 8 0ZM6.379 1.588C4.468 2.46 3 4.96 3 8c0 3.04 1.468 5.54 3.379 6.412A6.504 6.504 0 0 1 1.5 8c0-3.043 2.094-5.6 4.879-6.412ZM8 14.5c1.94 0 3.5-2.91 3.5-6.5S9.94 1.5 8 1.5 4.5 4.41 4.5 8s1.56 6.5 3.5 6.5Z",
  [SettingsTab.About]: "M0 8a8 8 0 1 1 16 0A8 8 0 0 1 0 8Zm8-6.5a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13ZM6.5 7.75A.75.75 0 0 1 7.25 7h1a.75.75 0 0 1 .75.75v2.75h.25a.75.75 0 0 1 0 1.5h-2a.75.75 0 0 1 0-1.5h.25v-2h-.25a.75.75 0 0 1-.75-.75ZM8 6a1 1 0 1 1 0-2 1 1 0 0 1 0 2Z",
};

const TABS: { id: SettingsTab; label: string }[] = [
  { id: SettingsTab.Appearance, label: "Appearance" },
  { id: SettingsTab.Terminal, label: "Terminal" },
  { id: SettingsTab.Keybindings, label: "Keybindings" },
  { id: SettingsTab.Notifications, label: "Notifications" },
  { id: SettingsTab.Agents, label: "Agents" },
  { id: SettingsTab.Network, label: "Network & API" },
  { id: SettingsTab.About, label: "About" },
];

export function SettingsPanel({
  config,
  onUpdateConfig,
  onClose,
}: SettingsPanelProps) {
  const [activeTab, setActiveTab] = useState(SettingsTab.Appearance);
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
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="w-[720px] h-[540px] bg-[var(--ct-sidebar-bg)] border border-[var(--ct-border)] rounded-xl shadow-2xl flex flex-col overflow-hidden relative">
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
            <svg width="10" height="10" viewBox="0 0 10 10" fill="currentColor">
              <path d="M1.41 0L5 3.59 8.59 0 10 1.41 6.41 5 10 8.59 8.59 10 5 6.41 1.41 10 0 8.59 3.59 5 0 1.41Z" />
            </svg>
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
            {activeTab === SettingsTab.Appearance && (
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
              </div>
            )}

            {activeTab === SettingsTab.Terminal && (
              <div className="space-y-6">
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
              </div>
            )}

            {activeTab === SettingsTab.Keybindings && (
              <div className="space-y-4">
                <KeybindGroup title="Tabs" bindings={[
                  ["new_tab", "New Tab"],
                  ["close_tab", "Close Tab"],
                ]} config={localConfig} onUpdate={(c) => { setLocalConfig(c); onUpdateConfig(c).catch(() => {}); }} />

                <KeybindGroup title="Panes" bindings={[
                  ["split_right", "Split Right"],
                  ["split_down", "Split Down"],
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
                  <button
                    onClick={async () => {
                      try {
                        const { isPermissionGranted, requestPermission, sendNotification } = await import("@tauri-apps/plugin-notification");
                        let granted = await isPermissionGranted();
                        if (!granted) {
                          const perm = await requestPermission();
                          granted = perm === "granted";
                        }
                        if (granted) {
                          sendNotification({ title: "CodeTerm", body: "Notifications are working!" });
                        }
                      } catch (e) {
                        console.error("Notification error:", e);
                      }
                    }}
                    className="mt-1 px-3 py-1 text-xs bg-[var(--ct-border)] hover:bg-[var(--ct-muted)] rounded transition-colors"
                  >
                    Send test notification
                  </button>
                </Group>

                <ClaudeHooksGroup />

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
              </div>
            )}

            {activeTab === SettingsTab.Agents && (
              <AgentsTab config={localConfig} onUpdate={(section, patch) => update(section, patch)} />
            )}

            {activeTab === SettingsTab.Network && (
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

function Group({ title, children }: { title: string; children: React.ReactNode }) {
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

function Toggle({ label, checked, onChange }: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="flex items-center justify-between cursor-pointer">
      <span className="text-xs text-[var(--ct-muted-fg)]">{label}</span>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        onClick={() => onChange(!checked)}
        className={`relative inline-flex h-5 w-9 items-center rounded-full transition-colors ${
          checked ? "bg-[var(--ct-green)]" : "bg-[var(--ct-border)]"
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

function CopyRow({ label, value }: { label: string; value: string }) {
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
            <svg width="10" height="10" viewBox="0 0 16 16" fill="currentColor"><path d="M13.78 4.22a.75.75 0 0 1 0 1.06l-7.25 7.25a.75.75 0 0 1-1.06 0L2.22 9.28a.75.75 0 0 1 1.06-1.06L6 10.94l6.72-6.72a.75.75 0 0 1 1.06 0Z"/></svg>
          ) : (
            <svg width="10" height="10" viewBox="0 0 16 16" fill="currentColor"><path d="M0 6.75C0 5.784.784 5 1.75 5h1.5a.75.75 0 0 1 0 1.5h-1.5a.25.25 0 0 0-.25.25v7.5c0 .138.112.25.25.25h7.5a.25.25 0 0 0 .25-.25v-1.5a.75.75 0 0 1 1.5 0v1.5A1.75 1.75 0 0 1 9.25 16h-7.5A1.75 1.75 0 0 1 0 14.25ZM5 1.75C5 .784 5.784 0 6.75 0h7.5C15.216 0 16 .784 16 1.75v7.5A1.75 1.75 0 0 1 14.25 11h-7.5A1.75 1.75 0 0 1 5 9.25Zm1.75-.25a.25.25 0 0 0-.25.25v7.5c0 .138.112.25.25.25h7.5a.25.25 0 0 0 .25-.25v-7.5a.25.25 0 0 0-.25-.25Z"/></svg>
          )}
        </button>
      </div>
    </div>
  );
}

function InfoRow({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
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

const IS_MAC_SETTINGS = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);

const KEY_SYMBOLS: Record<string, string> = IS_MAC_SETTINGS ? {
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
        const display = IS_MAC_SETTINGS && part.toLowerCase() === "ctrl" && parts.some(p => /^[a-zA-Z0-9]$/.test(p))
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
  const [copiedPrompt, setCopiedPrompt] = useState(false);
  const [tokenRefreshing, setTokenRefreshing] = useState(false);

  useEffect(() => {
    invoke<typeof net>("get_network_info").then(setNet).catch(() => {});
  }, []);

  const [gatewayPrompt, setgatewayPrompt] = useState("");

  const buildPrompt = useCallback((netInfo: typeof net, token: string | null) => {
    if (!netInfo) return;
    invoke<string>("get_skills_prompt").then((base) => {
      const url = netInfo.tailscale_url || netInfo.api_url;
      const authLine = token ? `\nAuthorization: Bearer ${token}` : "";
      setgatewayPrompt(`CodeTerm API: ${url}${authLine}\n\n${base}`);
    }).catch(() => {});
  }, []);

  useEffect(() => {
    buildPrompt(net, cfg.api?.token ?? null);
  }, [net, cfg.api?.token, buildPrompt]);

  const refreshToken = useCallback(async () => {
    setTokenRefreshing(true);
    try {
      const token = await invoke<string>("generate_api_token");
      onUpdate("api", { token });
      buildPrompt(net, token);
    } catch {}
    setTokenRefreshing(false);
  }, [net, onUpdate, buildPrompt]);

  return (
    <div className="space-y-6">
      <Group title="REST API">
        <Toggle
          label="Enable API"
          checked={cfg.api?.enabled ?? false}
          onChange={(v) => onUpdate("api", { enabled: v })}
        />
        <div className="text-[10px] text-[var(--ct-muted)] -mt-1">
          Requires restart to take effect.
        </div>
        <div className="space-y-1.5 mt-2">
          <CopyRow label="Local" value={net?.api_url || "..."} />
          {net?.tailscale_url && (
            <CopyRow label="Tailscale" value={net.tailscale_url} />
          )}
        </div>
      </Group>

      <Group title="API Token">
        <div className="text-[10px] text-[var(--ct-muted)] mb-2">
          Bearer token required for all API requests. Included in the orchestrator prompt automatically.
        </div>
        {cfg.api?.token ? (
          <div className="flex items-center gap-2">
            <CopyRow label="Token" value={cfg.api.token} />
            <button
              onClick={refreshToken}
              disabled={tokenRefreshing}
              className="shrink-0 px-2 py-1 text-[10px] rounded bg-[var(--ct-border)] text-[var(--ct-fg)] hover:bg-[var(--ct-muted)] transition-colors disabled:opacity-50"
            >
              {tokenRefreshing ? "..." : "Regenerate"}
            </button>
          </div>
        ) : (
          <button
            onClick={refreshToken}
            disabled={tokenRefreshing}
            className="px-3 py-1.5 text-[11px] rounded bg-[var(--ct-accent)] text-white hover:brightness-110 transition-all disabled:opacity-50"
          >
            {tokenRefreshing ? "Generating..." : "Generate Token"}
          </button>
        )}
      </Group>

      <Group title="Webhooks">
        <div className="text-xs text-[var(--ct-muted-fg)] space-y-1">
          <p>Register webhooks to receive real-time agent events. External orchestrators get instant feedback when agents need attention.</p>
          <CopyRow label="Endpoint" value="POST /api/webhooks" />
          <div className="text-[10px] text-[var(--ct-muted)]">
            Body: <code className="bg-[var(--ct-bg)] px-1 rounded">{"{ url, events? }"}</code>
          </div>
        </div>
      </Group>

      <Group title="a gateway / External Orchestrator">
        <div className="text-xs text-[var(--ct-muted-fg)] mb-2">
          Copy this prompt and give it to your external orchestrator (a gateway, etc.) so it knows how to control CodeTerm. {cfg.api?.token ? "Includes your API token." : "Generate a token above to include authentication."}
        </div>
        <div className="relative">
          <pre className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded p-3 text-[10px] text-[var(--ct-muted-fg)] font-mono whitespace-pre-wrap max-h-[160px] overflow-y-auto leading-relaxed">
            {gatewayPrompt || "Loading..."}
          </pre>
          <button
            onClick={() => {
              navigator.clipboard.writeText(gatewayPrompt).then(() => {
                setCopiedPrompt(true);
                setTimeout(() => setCopiedPrompt(false), 2000);
              });
            }}
            className="absolute top-2 right-2 px-2 py-1 text-[10px] rounded bg-[var(--ct-border)] text-[var(--ct-fg)] hover:bg-[var(--ct-muted)] transition-colors"
          >
            {copiedPrompt ? "Copied!" : "Copy"}
          </button>
        </div>
      </Group>

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
            ["POST", "/api/panes/:id/split", "Split {direction}"],
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
            ["POST", "/api/webhooks", "Register webhook {url, events}"],
            ["GET", "/api/webhooks", "List webhooks"],
            ["GET", "/api/network", "Connection info + IPs"],
            ["GET", "/api/screenshot", "Capture screenshot"],
            ["GET", "/api/serve-file", "Serve local file {?path=}"],
          ]} />
        </div>
      </Collapsible>
    </div>
  );
}

function AgentsTab({ config, onUpdate }: { config: AppConfig; onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void }) {
  const [skillInstalled, setSkillInstalled] = useState(false);
  const [skillUpdate, setSkillUpdate] = useState(false);
  const [ruleInstalled, setRuleInstalled] = useState(false);
  const [ruleUpdate, setRuleUpdate] = useState(false);
  const [skillJustDone, setSkillJustDone] = useState(false);
  const [ruleJustDone, setRuleJustDone] = useState(false);

  useEffect(() => {
    if (!skillJustDone) return;
    const t = setTimeout(() => setSkillJustDone(false), 2000);
    return () => clearTimeout(t);
  }, [skillJustDone]);

  useEffect(() => {
    if (!ruleJustDone) return;
    const t = setTimeout(() => setRuleJustDone(false), 2000);
    return () => clearTimeout(t);
  }, [ruleJustDone]);

  useEffect(() => {
    invoke<{ id: string; installed: boolean; update_available: boolean }[]>("list_skills").then((s) => {
      setSkillInstalled(s.some((x) => x.installed));
      setSkillUpdate(s.some((x) => x.update_available));
    }).catch(() => {});
    invoke<{ installed: boolean; update_available: boolean }>("check_rule_installed").then((r) => {
      setRuleInstalled(r.installed);
      setRuleUpdate(r.update_available);
    }).catch(() => {});
  }, []);

  const openFolder = useCallback((path: string) => {
    invoke("open_folder", { path }).catch(() => {});
  }, []);

  return (
    <div className="space-y-5">
      {/* Session resume settings */}
      <div className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4 space-y-3">
        <div className="text-sm font-medium text-[var(--ct-fg)]">Session Resume</div>
        <div className="text-xs text-[var(--ct-muted-fg)]">
          Automatically resume Claude Code sessions when CodeTerm restarts.
        </div>
        <Toggle
          label="Resume previous sessions"
          checked={config.agent.resume_sessions}
          onChange={(v) => onUpdate("agent", { resume_sessions: v })}
        />
        <Toggle
          label="Resume with --dangerously-skip-permissions"
          checked={config.agent.resume_skip_permissions}
          onChange={(v) => onUpdate("agent", { resume_skip_permissions: v })}
        />
        {config.agent.resume_skip_permissions && (
          <div className="text-[10px] text-[var(--ct-yellow)] flex items-center gap-1.5">
            <span>⚠</span>
            <span>Claude Code will run without asking for tool approvals. Use with caution.</span>
          </div>
        )}
      </div>

      {/* Skill card */}
      <div className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4 space-y-3">
        <div className="flex items-center justify-between">
          <div className="text-sm font-medium text-[var(--ct-fg)]">Skill</div>
          {skillInstalled && (
            <span className="text-[9px] px-1.5 py-0.5 rounded-full bg-[var(--ct-green)]/20 text-[var(--ct-green)]">installed</span>
          )}
        </div>
        <div className="text-xs text-[var(--ct-muted-fg)]">
          Gives Claude Code CLI commands to control CodeTerm — create workspaces, split panes, open browser panels, send notifications, rename tabs.
        </div>
        <div className="flex items-center justify-between text-[10px] text-[var(--ct-muted)]">
          <span>~/.claude/skills/codeterm/</span>
          <button
            onClick={() => openFolder("~/.claude/skills")}
            className="text-[var(--ct-accent)] hover:underline"
          >Open folder</button>
        </div>
        <button
          onClick={async () => {
            try {
              await invoke("install_skill", { skillId: "codeterm" });
              setSkillInstalled(true);
              setSkillUpdate(false);
              setSkillJustDone(true);
            } catch (e) {
              console.error("Skill install failed:", e);
            }
          }}
          disabled={skillJustDone}
          className={`w-full py-1.5 text-xs rounded font-medium transition-all duration-300 ${
            skillJustDone
              ? "bg-[var(--ct-green)] text-white"
              : skillUpdate
                ? "bg-[var(--ct-accent)] text-white hover:brightness-110"
                : skillInstalled
                  ? "border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]"
                  : "bg-[var(--ct-green)] text-white hover:brightness-110"
          }`}
        >
          {skillJustDone ? "\u2713 Installed" : skillUpdate ? "Update Available" : skillInstalled ? "Reinstall" : "Install"}
        </button>
      </div>

      {/* Rule card */}
      <div className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4 space-y-3">
        <div className="flex items-center justify-between">
          <div className="text-sm font-medium text-[var(--ct-fg)]">Rule</div>
          {ruleInstalled && (
            <span className="text-[9px] px-1.5 py-0.5 rounded-full bg-[var(--ct-green)]/20 text-[var(--ct-green)]">installed</span>
          )}
        </div>
        <div className="text-xs text-[var(--ct-muted-fg)]">
          Teaches Claude Code when and how to use CodeTerm — auto-rename tabs, use workspaces instead of mkdir, open browser panel instead of external browser.
        </div>
        <div className="flex items-center justify-between text-[10px] text-[var(--ct-muted)]">
          <span>~/.claude/rules/codeterm.md</span>
          <button
            onClick={() => openFolder("~/.claude/rules")}
            className="text-[var(--ct-accent)] hover:underline"
          >Open folder</button>
        </div>
        <button
          onClick={async () => {
            await invoke("install_project_rules");
            setRuleInstalled(true);
            setRuleUpdate(false);
            setRuleJustDone(true);
          }}
          disabled={ruleJustDone}
          className={`w-full py-1.5 text-xs rounded font-medium transition-all duration-300 ${
            ruleJustDone
              ? "bg-[var(--ct-green)] text-white"
              : ruleUpdate
                ? "bg-[var(--ct-accent)] text-white hover:brightness-110"
                : ruleInstalled
                  ? "border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]"
                  : "bg-[var(--ct-green)] text-white hover:brightness-110"
          }`}
        >
          {ruleJustDone ? "\u2713 Installed" : ruleUpdate ? "Update Available" : ruleInstalled ? "Reinstall" : "Install"}
        </button>
      </div>

      <div className="text-[10px] text-[var(--ct-muted)] text-center">
        Restart Claude Code after installing.
      </div>
    </div>
  );
}

function ClaudeHooksGroup() {
  const [status, setStatus] = useState<{
    settings_exists: boolean;
    hooks_installed: boolean;
    notification_hook: boolean;
    stop_hook: boolean;
  } | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    invoke<typeof status>("check_claude_hooks").then(setStatus).catch(() => {});
  }, []);

  const handleInstall = useCallback(async () => {
    setLoading(true);
    try {
      await invoke("install_claude_hooks", {});
      const s = await invoke<typeof status>("check_claude_hooks");
      setStatus(s);
    } catch (e) {
      console.error("Failed to install hooks:", e);
    }
    setLoading(false);
  }, []);

  const handleUninstall = useCallback(async () => {
    setLoading(true);
    try {
      await invoke("uninstall_claude_hooks");
      const s = await invoke<typeof status>("check_claude_hooks");
      setStatus(s);
    } catch (e) {
      console.error("Failed to uninstall hooks:", e);
    }
    setLoading(false);
  }, []);

  return (
    <Group title="Claude Code Notifications">
      <div className="text-xs text-[var(--ct-muted-fg)] mb-2">
        Get notified when Claude Code needs permission or finishes a task.
        This installs hooks into <span className="font-mono text-[var(--ct-fg)]">~/.claude/settings.json</span>.
      </div>

      {status === null ? (
        <div className="text-xs text-[var(--ct-muted)]">Checking...</div>
      ) : status.hooks_installed ? (
        <>
          <div className="flex items-center gap-2 mb-2">
            <span className="w-2 h-2 rounded-full bg-[var(--ct-green)]" />
            <span className="text-xs text-[var(--ct-fg)]">Hooks installed</span>
          </div>
          <div className="text-[10px] text-[var(--ct-muted)] space-y-0.5 mb-2">
            <div className="flex items-center gap-1.5">
              <span>{status.notification_hook ? "\u2713" : "\u2717"}</span>
              <span>Permission requests</span>
            </div>
            <div className="flex items-center gap-1.5">
              <span>{status.stop_hook ? "\u2713" : "\u2717"}</span>
              <span>Task completion</span>
            </div>
          </div>
          <button
            onClick={handleUninstall}
            disabled={loading}
            className="w-full py-1.5 text-xs rounded font-medium border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] transition-colors"
          >
            {loading ? "..." : "Uninstall Hooks"}
          </button>
        </>
      ) : (
        <>
          <div className="flex items-center gap-2 mb-2">
            <span className="w-2 h-2 rounded-full bg-[var(--ct-muted)]" />
            <span className="text-xs text-[var(--ct-muted-fg)]">Hooks not installed</span>
          </div>
          <div className="text-[10px] text-[var(--ct-muted)] mb-2">
            {!status.settings_exists
              ? "~/.claude/settings.json does not exist yet. It will be created."
              : "CodeTerm hooks will be added to your existing Claude Code settings."}
          </div>
          <button
            onClick={handleInstall}
            disabled={loading}
            className="w-full py-1.5 text-xs rounded font-medium bg-[var(--ct-accent)] text-white hover:brightness-110 transition-colors"
          >
            {loading ? "Installing..." : "Install Claude Hooks"}
          </button>
        </>
      )}

      <div className="text-[10px] text-[var(--ct-muted)] mt-2">
        Restart Claude Code after changes for hooks to take effect.
      </div>
    </Group>
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
