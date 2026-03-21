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
  [SettingsTab.Appearance]: "M0 2.75C0 1.784.784 1 1.75 1h12.5c.966 0 1.75.784 1.75 1.75v10.5A1.75 1.75 0 0 1 14.25 15H1.75A1.75 1.75 0 0 1 0 13.25Z",
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
        const next = { ...prev, [section]: { ...prev[section], ...patch } };
        onUpdateConfig(next).catch(() => {});
        return next;
      });
    },
    [onUpdateConfig],
  );

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="w-[720px] h-[520px] bg-[var(--ct-sidebar-bg)] border border-[var(--ct-border)] rounded-lg shadow-xl flex flex-col overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-2.5 border-b border-[var(--ct-border)]">
          <h2 className="text-sm font-semibold text-[var(--ct-fg)]">Settings</h2>
          <button
            onClick={onClose}
            className="flex items-center justify-center w-6 h-6 rounded hover:bg-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] transition-colors"
          >
            <svg width="10" height="10" viewBox="0 0 10 10" fill="currentColor">
              <path d="M1.41 0L5 3.59 8.59 0 10 1.41 6.41 5 10 8.59 8.59 10 5 6.41 1.41 10 0 8.59 3.59 5 0 1.41Z" />
            </svg>
          </button>
        </div>

        {/* Body: sidebar tabs + content */}
        <div className="flex flex-1 min-h-0">
          {/* Tab sidebar */}
          <div className="w-[170px] shrink-0 border-r border-[var(--ct-border)] py-2 flex flex-col gap-0.5">
            {TABS.map((tab) => (
              <button
                key={tab.id}
                onClick={() => setActiveTab(tab.id)}
                className={`flex items-center gap-2 px-4 py-1.5 text-xs transition-colors ${
                  activeTab === tab.id
                    ? "bg-[var(--ct-active)] text-[var(--ct-fg)] border-r-2 border-[var(--ct-accent)]"
                    : "text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"
                }`}
              >
                <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor" className="shrink-0 opacity-70">
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
                  ["next_tab", "Next Tab"],
                  ["prev_tab", "Previous Tab"],
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
                  ["copy", "Copy"],
                  ["paste", "Paste"],
                  ["search", "Search"],
                  ["zoom_in", "Zoom In"],
                  ["zoom_out", "Zoom Out"],
                  ["zoom_reset", "Zoom Reset"],
                  ["open_browser", "Open Browser"],
                  ["open_settings", "Settings"],
                ]} config={localConfig} onUpdate={(c) => { setLocalConfig(c); onUpdateConfig(c).catch(() => {}); }} />
              </div>
            )}

            {activeTab === SettingsTab.Notifications && (
              <div className="space-y-6">
                <Group title="Windows Notifications">
                  <div className="text-xs text-[var(--ct-muted-fg)] mb-1">
                    Show OS-level toast notifications when CodeTerm window is not focused.
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

                <Group title="Agent Events">
                  <div className="text-xs text-[var(--ct-muted-fg)] mb-1">
                    Automatically notify when Claude Code or other agents need attention.
                  </div>
                  <Toggle
                    label="Permission requests"
                    checked={localConfig.notifications.desktop}
                    onChange={(v) => update("notifications", { desktop: v })}
                  />
                  <Toggle
                    label="Task completion"
                    checked={localConfig.notifications.desktop}
                    onChange={(v) => update("notifications", { desktop: v })}
                  />
                  <div className="text-[10px] text-[var(--ct-muted)] mt-1 space-y-1">
                    <p><strong className="text-[var(--ct-muted-fg)]">Permission requests</strong> — when Claude Code asks "Do you want to proceed?" or needs tool approval</p>
                    <p><strong className="text-[var(--ct-muted-fg)]">Task completion</strong> — when Claude Code finishes working and returns to the prompt</p>
                  </div>
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
              <div className="space-y-4">
                <div className="text-center py-4">
                  <div className="text-2xl font-bold text-[var(--ct-fg)]">CodeTerm</div>
                  <div className="text-xs text-[var(--ct-muted-fg)] mt-1">AI-first terminal multiplexer</div>
                </div>
                <Group title="Info">
                  <InfoRow label="Version" value="0.1.0" />
                  <InfoRow label="Platform" value={navigator.platform} />
                  <InfoRow
                    label="Config"
                    value={configPath || "loading..."}
                    mono
                  />
                </Group>
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
      <h3 className="text-[11px] font-semibold uppercase tracking-wider text-[var(--ct-muted)] mb-2">{title}</h3>
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
  return (
    <Group title={title}>
      <div className="space-y-1">
        {bindings.map(([key, label]) => (
          <div key={key} className="flex items-center justify-between">
            <span className="text-xs text-[var(--ct-muted-fg)]">{label}</span>
            <input
              type="text"
              value={(config.keybindings as Record<string, string>)[key] || ""}
              onChange={(e) => {
                const next = {
                  ...config,
                  keybindings: { ...config.keybindings, [key]: e.target.value },
                };
                onUpdate(next);
              }}
              className="w-32 bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-2 py-0.5 text-[11px] text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none text-right font-mono"
            />
          </div>
        ))}
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

  useEffect(() => {
    invoke<typeof net>("get_network_info").then(setNet).catch(() => {});
  }, []);

  const [gatewayPrompt, setgatewayPrompt] = useState("");

  useEffect(() => {
    if (!net) return;
    invoke<string>("get_skills_prompt").then((base) => {
      const url = net.tailscale_url || net.api_url;
      setgatewayPrompt(`CodeTerm API: ${url}\n\n${base}`);
    }).catch(() => {});
  }, [net]);

  return (
    <div className="space-y-6">
      <Group title="REST API">
        <Toggle
          label="Enable API"
          checked={cfg.api?.enabled ?? true}
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
          Copy this prompt and give it to your external orchestrator (a gateway, etc.) so it knows how to control CodeTerm:
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
            onClick={() => openFolder("~/.claude/skills/codeterm")}
            className="text-[var(--ct-accent)] hover:underline"
          >Open folder</button>
        </div>
        <button
          onClick={async () => {
            await invoke("install_skill", { skillId: "codeterm" });
            setSkillInstalled(true);
            setSkillUpdate(false);
          }}
          className={`w-full py-1.5 text-xs rounded font-medium transition-colors ${
            skillUpdate
              ? "bg-[var(--ct-accent)] text-white hover:brightness-110"
              : skillInstalled
                ? "border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]"
                : "bg-[var(--ct-green)] text-white hover:brightness-110"
          }`}
        >
          {skillUpdate ? "Update Available" : skillInstalled ? "Reinstall" : "Install"}
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
          }}
          className={`w-full py-1.5 text-xs rounded font-medium transition-colors ${
            ruleUpdate
              ? "bg-[var(--ct-accent)] text-white hover:brightness-110"
              : ruleInstalled
                ? "border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]"
                : "bg-[var(--ct-green)] text-white hover:brightness-110"
          }`}
        >
          {ruleUpdate ? "Update Available" : ruleInstalled ? "Reinstall" : "Install"}
        </button>
      </div>

      <div className="text-[10px] text-[var(--ct-muted)] text-center">
        Restart Claude Code after installing.
      </div>
    </div>
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
