import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AppConfig } from "../../lib/types";
import { CloseXIcon } from "../Icons";
import {
  Sliders,
  Database,
  Bot,
  Bell,
  Plug,
  Network,
  Info,
  KeyRound,
  ChevronLeft,
  ChevronRight,
  ExternalLink,
  type LucideIcon,
} from "lucide-react";

import { Group, Toggle, InfoRow, AboutLink } from "./shared";
import { KeybindGroup } from "./KeybindGroup";
import { UpdateSection } from "./UpdateSection";
import { ShellDiagnosticsBanner } from "./ShellDiagnosticsBanner";
import { ProviderNotifications } from "./ProviderNotifications";
import { MemoryTab } from "./MemoryTab";
import { AgentsOverview, AgentProviderScreen } from "./AgentsTab";
import { IntegrationsTab } from "./IntegrationsTab";
import { SecretsTab } from "./SecretsTab";
import { NetworkTab } from "./NetworkTab";

interface SettingsPanelProps {
  config: AppConfig;
  onUpdateConfig: (config: AppConfig) => Promise<void>;
  onClose: () => void;
  initialSection?: SectionId;
}

export type SectionId =
  | "general"
  | "memory"
  | "agents"
  | "notifications"
  | "integrations"
  | "secrets"
  | "mesh"
  | "about";

type Route =
  | { kind: "root" }
  | { kind: "section"; id: SectionId }
  | { kind: "agent-provider"; providerId: string };

const SECTION_ICONS: Record<SectionId, LucideIcon> = {
  general: Sliders,
  memory: Database,
  agents: Bot,
  notifications: Bell,
  integrations: Plug,
  secrets: KeyRound,
  mesh: Network,
  about: Info,
};

const SECTIONS: { id: SectionId; label: string; desc: string }[] = [
  { id: "general", label: "General", desc: "Theme, fonts, cursor, keybindings" },
  { id: "memory", label: "Memory", desc: "Extraction workers and nightly dreaming" },
  { id: "agents", label: "Agents", desc: "General Agent and per-provider config" },
  { id: "notifications", label: "Notifications", desc: "Desktop, sound, in-app toasts" },
  { id: "integrations", label: "Integrations", desc: "Telegram, Discord, external bridges" },
  { id: "secrets", label: "Secrets", desc: "Bitwarden, Keychain — pluggable secret store" },
  { id: "mesh", label: "Mesh", desc: "Peer discovery, network API reference" },
  { id: "about", label: "About", desc: "Version, updates, creator" },
];

function sectionTitle(id: SectionId): string {
  return SECTIONS.find((s) => s.id === id)?.label ?? id;
}

export function SettingsPanel({
  config,
  onUpdateConfig,
  onClose,
  initialSection,
}: SettingsPanelProps) {
  const [stack, setStack] = useState<Route[]>(
    initialSection
      ? [{ kind: "root" }, { kind: "section", id: initialSection }]
      : [{ kind: "root" }],
  );
  const top = stack[stack.length - 1];
  const push = useCallback((r: Route) => setStack((s) => [...s, r]), []);
  const pop = useCallback(
    () => setStack((s) => (s.length > 1 ? s.slice(0, -1) : s)),
    [],
  );
  useEffect(() => {
    if (initialSection) {
      setStack([{ kind: "root" }, { kind: "section", id: initialSection }]);
    }
  }, [initialSection]);

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

  const headerTitle =
    top.kind === "root"
      ? "Settings"
      : top.kind === "section"
      ? sectionTitle(top.id)
      : "Provider";

  return (
    <div className="absolute inset-0 flex flex-col overflow-hidden bg-[var(--ct-sidebar-bg)]">
      <div className="absolute inset-0 flex flex-col">
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
          <div className="flex items-center gap-2 min-w-0">
            {top.kind !== "root" && (
              <button
                onClick={pop}
                className="flex items-center gap-1 px-2 h-7 rounded-md hover:bg-[var(--ct-hover)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] transition-colors text-[11px]"
                title="Back"
              >
                <ChevronLeft size={14} className="shrink-0" />
                <span>
                  {stack.length >= 2 && stack[stack.length - 2].kind === "root"
                    ? "Settings"
                    : stack.length >= 2 && stack[stack.length - 2].kind === "section"
                    ? sectionTitle((stack[stack.length - 2] as { kind: "section"; id: SectionId }).id)
                    : "Back"}
                </span>
              </button>
            )}
            <h2 className="text-sm font-medium text-[var(--ct-fg)] truncate">{headerTitle}</h2>
          </div>
          <button
            onClick={onClose}
            className="flex items-center justify-center w-7 h-7 rounded-md hover:bg-[var(--ct-hover)] text-[var(--ct-muted)] hover:text-[var(--ct-fg)] transition-colors"
          >
            <CloseXIcon />
          </button>
        </div>

        {/* Body */}
        <div className="flex flex-1 min-h-0 relative z-10">
          <div className="flex-1 overflow-y-auto px-5 py-5">
            {top.kind === "root" && (
              <div className="max-w-3xl mx-auto">
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                  {SECTIONS.map((s) => {
                    const Icon = SECTION_ICONS[s.id];
                    return (
                      <button
                        key={s.id}
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
              <div className="max-w-2xl mx-auto space-y-6">
                <ShellDiagnosticsBanner onDismissSettings={onClose} />
                <Group title="Color Theme">
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
                </Group>

                <Group title="Cursor">
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
                </Group>

                <Group title="Scrollback">
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

            {top.kind === "section" && top.id === "notifications" && (
              <div className="max-w-2xl mx-auto space-y-6">
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

            {top.kind === "section" && top.id === "memory" && (
              <div className="max-w-2xl mx-auto">
                <MemoryTab config={localConfig} onUpdate={(section, patch) => update(section, patch)} />
              </div>
            )}

            {top.kind === "section" && top.id === "agents" && (
              <div className="max-w-2xl mx-auto">
                <AgentsOverview
                  config={localConfig}
                  onUpdate={(section, patch) => update(section, patch)}
                  onOpenProvider={(providerId) => push({ kind: "agent-provider", providerId })}
                />
              </div>
            )}

            {top.kind === "agent-provider" && (
              <div className="max-w-2xl mx-auto">
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
              <div className="max-w-2xl mx-auto">
                <IntegrationsTab config={localConfig} onUpdate={(section, patch) => update(section, patch)} />
              </div>
            )}

            {top.kind === "section" && top.id === "secrets" && (
              <div className="max-w-2xl mx-auto">
                <SecretsTab
                  config={localConfig}
                  onUpdate={(section, patch) => update(section, patch)}
                  onCloseSettings={onClose}
                />
              </div>
            )}

            {top.kind === "section" && top.id === "mesh" && (
              <div className="max-w-2xl mx-auto">
                <NetworkTab
                  config={localConfig}
                  onUpdate={(section, patch) => update(section, patch)}
                />
              </div>
            )}

            {top.kind === "section" && top.id === "about" && (
              <div className="max-w-2xl mx-auto space-y-6">
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

                <UpdateSection />

                {/* Creator */}
                <Group title="Created by">
                  <div className="text-sm text-[var(--ct-fg)]">Andrey Kovalev</div>
                  <div className="flex flex-wrap gap-3 mt-2">
                    <AboutLink href="https://github.com/rollacode" label="GitHub" Icon={ExternalLink} />
                    <AboutLink href="https://rollacode.org" label="Website" Icon={ExternalLink} />
                    <AboutLink href="https://www.linkedin.com/in/rollacode" label="LinkedIn" Icon={ExternalLink} />
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
