import { useState, useEffect, useCallback } from "react";
import { invoke } from "../../lib/bridge";
import type { AppConfig } from "../../lib/types";
import {
  Sliders,
  Bot,
  Plug,
  Info,
  KeyRound,
  LayoutGrid,
  ChevronLeft,
  ChevronRight,
  ExternalLink,
  Palette,
  Type,
  MousePointer,
  ScrollText,
  Mouse,
  Power,
  Keyboard,
  FileText,
  Package,
  type LucideIcon,
} from "lucide-react";

import logoUrl from "../../assets/logo.png";
import { Toggle, InfoRow, AboutLink, SettingsPage, SettingsCard } from "./shared";
import { KeybindGroup } from "./KeybindGroup";
import { setDefaultNewAgentPaneToChat } from "../../lib/chatUiMode";
import { UpdateSection } from "./UpdateSection";
import { ShellDiagnosticsBanner } from "./ShellDiagnosticsBanner";
import { AgentsOverview, AgentProviderScreen } from "./AgentsTab";
import { IntegrationsTab } from "./IntegrationsTab";
import { SecretsTab } from "./SecretsTab";
import { LogsTab } from "./LogsTab";
import { PluginsTab } from "./PluginsTab";
import { UI_PANEL } from "../shared/design";

interface SettingsPanelProps {
  config: AppConfig;
  onUpdateConfig: (config: AppConfig) => Promise<void>;
  onClose: () => void;
  initialSection?: SectionId | "notifications";
  onNavigateToTab?: (tab: "memory" | "mesh") => void;
}

export type SectionId =
  | "general"
  | "tabs"
  | "agents"
  | "plugins"
  | "integrations"
  | "secrets"
  | "logs"
  | "about";

type Route =
  | { kind: "root" }
  | { kind: "section"; id: SectionId }
  | { kind: "agent-provider"; providerId: string };

const SECTION_ICONS: Record<SectionId, LucideIcon> = {
  general: Sliders,
  tabs: LayoutGrid,
  agents: Bot,
  plugins: Package,
  integrations: Plug,
  secrets: KeyRound,
  logs: FileText,
  about: Info,
};

const SECTIONS: { id: SectionId; label: string; desc: string }[] = [
  { id: "general", label: "General", desc: "Theme, fonts, cursor, keybindings" },
  { id: "tabs", label: "Tabs", desc: "Toggle Memory and Mesh tabs in the sidebar" },
  { id: "agents", label: "Agents", desc: "General Agent, notifications, and per-provider config" },
  // Plugins live in the first-class Extensions surface now (titlebar ⊞ / sidebar).
  { id: "integrations", label: "Integrations", desc: "Telegram, Discord, external bridges" },
  // Secret store is managed in the Bitwarden extension now (Extensions surface).
  { id: "logs", label: "Logs", desc: "App diagnostics — live, filterable, scoped" },
  { id: "about", label: "About", desc: "Version, updates, creator" },
];

function normalizeSection(id: SectionId | "notifications" | undefined): SectionId | undefined {
  if (!id) return undefined;
  return id === "notifications" ? "agents" : id;
}

function sectionTitle(id: SectionId): string {
  return SECTIONS.find((s) => s.id === id)?.label ?? id;
}

export function SettingsPanel({
  config,
  onUpdateConfig,
  onClose,
  initialSection,
  onNavigateToTab,
}: SettingsPanelProps) {
  const normalizedInitial = normalizeSection(initialSection);
  const [stack, setStack] = useState<Route[]>(
    normalizedInitial
      ? [{ kind: "root" }, { kind: "section", id: normalizedInitial }]
      : [{ kind: "root" }],
  );
  const top = stack[stack.length - 1];
  const push = useCallback((r: Route) => setStack((s) => [...s, r]), []);
  const pop = useCallback(
    () => setStack((s) => (s.length > 1 ? s.slice(0, -1) : s)),
    [],
  );
  useEffect(() => {
    const n = normalizeSection(initialSection);
    if (n) {
      setStack([{ kind: "root" }, { kind: "section", id: n }]);
    }
  }, [initialSection]);

  const [localConfig, setLocalConfig] = useState<AppConfig>(config);
  const [themes, setThemes] = useState<string[]>([]);
  const [themesDir, setThemesDir] = useState<string>("");
  const [autostartEnabled, setAutostartEnabled] = useState<boolean | null>(null);
  const chatUiDefault = localConfig.ui?.default_new_agent_pane_chat ?? false;

  const builtinThemes = ["github-dark", "dracula", "catppuccin-mocha", "one-dark", "nord", "github-light", "solarized-light", "pip-boy"];

  const refreshThemes = useCallback(() => {
    invoke<string[]>("list_themes").then(setThemes).catch(() => setThemes(builtinThemes));
  }, []);

  useEffect(() => {
    setLocalConfig(config);
  }, [config]);

  useEffect(() => {
    refreshThemes();
    invoke<string>("get_themes_dir").then(setThemesDir).catch(() => {});
    invoke<boolean>("get_autostart_enabled")
      .then(setAutostartEnabled)
      .catch(() => setAutostartEnabled(false));
  }, []);

  const handleAutostartToggle = useCallback(async (v: boolean) => {
    setAutostartEnabled(v);
    try {
      await invoke("set_autostart_enabled", { enabled: v });
    } catch (e) {
      console.error("set_autostart_enabled failed", e);
      // Re-read actual state so the toggle reflects truth on failure.
      invoke<boolean>("get_autostart_enabled")
        .then(setAutostartEnabled)
        .catch(() => {});
    }
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

  const headerTitle =
    top.kind === "root"
      ? "Settings"
      : top.kind === "section"
      ? sectionTitle(top.id)
      : "Provider";

  return (
    <div className="absolute inset-0 flex flex-col overflow-hidden bg-[var(--ct-sidebar-bg)]">
      <div className="absolute inset-0 flex flex-col pt-[var(--ct-safe-top,0px)] pb-[var(--ct-safe-bottom,0px)]">
        {/* Subtle dot grid background */}
        <div
          className="absolute inset-0 pointer-events-none"
          style={{
            backgroundImage: "radial-gradient(var(--ct-border) 1px, transparent 1px)",
            backgroundSize: "20px 20px",
            opacity: 0.3,
          }}
        />
        {/* Header — iOS style: back (chevron + parent label) left, title centered. */}
        <div className={`${UI_PANEL.toolbar} z-10 relative`}>
          <div className="flex min-w-0 flex-1 items-center">
            {top.kind !== "root" && (
              <button
                onClick={pop}
                className="-ml-1 flex items-center gap-0.5 pr-2 text-[var(--ct-accent)] transition-opacity active:opacity-60"
                title="Back"
              >
                <ChevronLeft size={19} strokeWidth={2.4} className="shrink-0" />
                <span className="truncate text-sm">
                  {stack.length >= 2 && stack[stack.length - 2].kind === "root"
                    ? "Settings"
                    : stack.length >= 2 && stack[stack.length - 2].kind === "section"
                    ? sectionTitle((stack[stack.length - 2] as { kind: "section"; id: SectionId }).id)
                    : "Back"}
                </span>
              </button>
            )}
          </div>
          <h2 className="pointer-events-none absolute left-1/2 max-w-[55%] -translate-x-1/2 truncate text-center text-sm font-semibold text-[var(--ct-fg)]">
            {headerTitle}
          </h2>
          {/* Balances the back button so the centered title stays optically centered. */}
          <div className="w-8 shrink-0" />
        </div>

        {/* Body */}
        <div className="flex flex-1 min-h-0 relative z-10">
          <div className="flex-1 overflow-y-auto px-5 py-5">
            {top.kind === "root" && (
              <div className="min-h-full flex items-center justify-center">
                <div className="w-full max-w-3xl grid grid-cols-1 sm:grid-cols-2 gap-3">
                  {SECTIONS.map((s) => {
                    const Icon = SECTION_ICONS[s.id];
                    return (
                      <button
                        key={s.id}
                        data-testid={`settings-section-${s.id}`}
                        onClick={() => push({ kind: "section", id: s.id })}
                        className="flex items-start gap-3 p-4 rounded-xl border border-[var(--ct-border)] bg-[var(--ct-bg)]/40 hover:bg-[var(--ct-hover)] hover:border-[var(--ct-accent)]/50 transition-colors text-left"
                      >
                        <div className="w-9 h-9 rounded-lg flex items-center justify-center bg-[var(--ct-accent)]/15 text-[var(--ct-accent)] shrink-0">
                          <Icon size={18} strokeWidth={2} />
                        </div>
                        <div className="min-w-0 flex-1">
                          <div className="text-[13px] font-medium text-[var(--ct-fg)]">{s.label}</div>
                          <div className="text-[11px] text-[var(--ct-muted-fg)] mt-0.5 line-clamp-2">{s.desc}</div>
                        </div>
                        <ChevronRight size={14} className="shrink-0 text-[var(--ct-muted)] mt-1" />
                      </button>
                    );
                  })}
                </div>
              </div>
            )}

            {top.kind === "section" && top.id === "general" && (
              <SettingsPage title="General" subtitle="Theme, fonts, cursor, behavior, and keybindings.">
                <ShellDiagnosticsBanner onDismissSettings={onClose} />

                <SettingsCard icon={<Palette size={16} />} title="Color Theme">
                  <label className="block">
                    <span className="block text-xs text-[var(--ct-muted-fg)] mb-1">Theme</span>
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
                  </label>
                  <div className="flex items-center gap-2">
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
                    <div className="flex items-center justify-between">
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
                </SettingsCard>

                <SettingsCard icon={<Type size={16} />} title="Font">
                  <label className="block">
                    <span className="block text-xs text-[var(--ct-muted-fg)] mb-1">Font family</span>
                    <input
                      type="text"
                      value={localConfig.font.family}
                      onChange={(e) => update("font", { family: e.target.value })}
                      className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
                    />
                  </label>
                  <label className="block">
                    <span className="block text-xs text-[var(--ct-muted-fg)] mb-1">{`Size: ${localConfig.font.size}px`}</span>
                    <input
                      type="range"
                      min={8}
                      max={28}
                      value={localConfig.font.size}
                      onChange={(e) => update("font", { size: parseInt(e.target.value, 10) })}
                      className="w-full accent-[var(--ct-accent)]"
                    />
                  </label>
                  <Toggle
                    label="Ligatures"
                    checked={localConfig.font.ligatures}
                    onChange={(v) => update("font", { ligatures: v })}
                  />
                </SettingsCard>

                <SettingsCard icon={<MousePointer size={16} />} title="Cursor">
                  <label className="block">
                    <span className="block text-xs text-[var(--ct-muted-fg)] mb-1">Style</span>
                    <select
                      value={localConfig.terminal.cursor_style}
                      onChange={(e) => update("terminal", { cursor_style: e.target.value })}
                      className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
                    >
                      <option value="block">Block</option>
                      <option value="underline">Underline</option>
                      <option value="bar">Bar</option>
                    </select>
                  </label>
                  <Toggle
                    label="Blink"
                    checked={localConfig.terminal.cursor_blink}
                    onChange={(v) => update("terminal", { cursor_blink: v })}
                  />
                </SettingsCard>

                <SettingsCard icon={<ScrollText size={16} />} title="Scrollback">
                  <label className="block">
                    <span className="block text-xs text-[var(--ct-muted-fg)] mb-1">{`Buffer: ${localConfig.terminal.scrollback.toLocaleString()} lines`}</span>
                    <input
                      type="range"
                      min={1000}
                      max={100000}
                      step={1000}
                      value={localConfig.terminal.scrollback}
                      onChange={(e) => update("terminal", { scrollback: parseInt(e.target.value, 10) })}
                      className="w-full accent-[var(--ct-accent)]"
                    />
                  </label>
                </SettingsCard>

                <SettingsCard icon={<Mouse size={16} />} title="Behavior">
                  <Toggle
                    label="Copy on select"
                    checked={localConfig.terminal.copy_on_select}
                    onChange={(v) => update("terminal", { copy_on_select: v })}
                  />
                  <Toggle
                    label="Open agents in Chat UI by default"
                    checked={chatUiDefault}
                    onChange={async (v) => {
                      update("ui", { ...localConfig.ui, default_new_agent_pane_chat: v });
                      await setDefaultNewAgentPaneToChat(v);
                    }}
                  />
                </SettingsCard>

                <SettingsCard
                  icon={<Power size={16} />}
                  title="Startup"
                  description="Adds CodeTerm to the OS login items so the daemon, mesh, and crons resume without you opening the app manually."
                >
                  <Toggle
                    label="Launch CodeTerm on system start"
                    checked={autostartEnabled ?? false}
                    onChange={handleAutostartToggle}
                  />
                  <Toggle
                    label="Enable Librarian (background skill vectorizer)"
                    checked={localConfig.memory?.librarian_enabled ?? true}
                    onChange={(v) => update("memory", { librarian_enabled: v })}
                  />
                </SettingsCard>

                <SettingsCard icon={<Keyboard size={16} />} title="Keybindings">
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
                </SettingsCard>
              </SettingsPage>
            )}

            {top.kind === "section" && top.id === "tabs" && (
              <SettingsPage title="Tabs" subtitle="Toggle Memory and Mesh in the sidebar.">
                <SettingsCard icon={<LayoutGrid size={16} />} title="Sidebar Tabs">
                  <Toggle
                    label="Enable Memory tab"
                    checked={localConfig.ui?.memory_tab_enabled ?? true}
                    onChange={(v) => update("ui", { memory_tab_enabled: v })}
                  />
                  <button
                    onClick={() => { onNavigateToTab?.("memory"); onClose(); }}
                    className="text-[var(--ct-accent)] text-[10px] hover:underline ml-6"
                  >
                    Go to Memory →
                  </button>
                  <Toggle
                    label="Enable Mesh tab"
                    checked={localConfig.ui?.mesh_tab_enabled ?? true}
                    onChange={(v) => {
                      update("ui", { mesh_tab_enabled: v });
                      if (v && !localConfig.mesh?.enabled) update("mesh", { enabled: true });
                    }}
                  />
                  <button
                    onClick={() => { onNavigateToTab?.("mesh"); onClose(); }}
                    className="text-[var(--ct-accent)] text-[10px] hover:underline ml-6"
                  >
                    Go to Mesh →
                  </button>
                </SettingsCard>
              </SettingsPage>
            )}

            {top.kind === "section" && top.id === "agents" && (
              <SettingsPage title="Agents" subtitle="General Agent, notifications, and per-provider configuration.">
                <AgentsOverview
                  config={localConfig}
                  onUpdate={(section, patch) => update(section, patch)}
                  onOpenProvider={(providerId) => push({ kind: "agent-provider", providerId })}
                  onOpenPlugins={() => push({ kind: "section", id: "plugins" })}
                  onUpdateFullConfig={(next) => {
                    setLocalConfig(next);
                    onUpdateConfig(next).catch(() => {});
                  }}
                />
              </SettingsPage>
            )}

            {top.kind === "section" && top.id === "plugins" && (
              <PluginsTab />
            )}

            {top.kind === "agent-provider" && (
              <div className="max-w-[720px] mx-auto">
                <AgentProviderScreen
                  providerId={top.providerId}
                  fullConfig={localConfig}
                  onUpdateFullConfig={(next) => {
                    setLocalConfig(next);
                    onUpdateConfig(next).catch(() => {});
                  }}
                  onProviderGone={pop}
                />
              </div>
            )}

            {top.kind === "section" && top.id === "integrations" && (
              <SettingsPage title="Integrations" subtitle="Telegram, Discord, and external bridges.">
                <IntegrationsTab config={localConfig} onUpdate={(section, patch) => update(section, patch)} />
              </SettingsPage>
            )}

            {top.kind === "section" && top.id === "secrets" && (
              <SettingsPage title="Secrets" subtitle="Pluggable secret store for the codeterm mem secret commands.">
                <SecretsTab
                  config={localConfig}
                  onUpdate={(section, patch) => update(section, patch)}
                  onCloseSettings={onClose}
                />
              </SettingsPage>
            )}

            {top.kind === "section" && top.id === "logs" && (
              <SettingsPage title="Logs" subtitle="App diagnostics from the daemon, mesh, agents, and integrations.">
                <LogsTab config={localConfig} />
              </SettingsPage>
            )}

            {top.kind === "section" && top.id === "about" && (
              <SettingsPage title="About" subtitle="Version, updates, and creator.">
                <div className="text-center py-4">
                  <img src={logoUrl} alt="CodeTerm" width="64" height="64" className="rounded-xl mb-3 mx-auto" />
                  <div className="text-xl font-bold text-[var(--ct-fg)]">CodeTerm</div>
                  <div className="text-xs text-[var(--ct-muted-fg)] mt-1">AI-first terminal multiplexer</div>
                </div>

                <SettingsCard icon={<Info size={16} />} title="Info">
                  <InfoRow label="Version" value={__APP_VERSION__} />
                </SettingsCard>

                <UpdateSection />

                <SettingsCard icon={<ExternalLink size={16} />} title="Created by">
                  <div className="text-xs text-[var(--ct-fg)]">Andrey Kovalev</div>
                  <div className="flex flex-wrap gap-3">
                    <AboutLink href="https://github.com/rollacode" label="GitHub" Icon={ExternalLink} />
                    <AboutLink href="https://rollacode.org" label="Website" Icon={ExternalLink} />
                    <AboutLink href="https://www.linkedin.com/in/rollacode" label="LinkedIn" Icon={ExternalLink} />
                  </div>
                </SettingsCard>

              </SettingsPage>
            )}
          </div>
        </div>

      </div>
    </div>
  );
}
