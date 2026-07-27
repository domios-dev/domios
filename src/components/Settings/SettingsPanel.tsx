import { useState, useEffect, useCallback } from "react";
import { invoke } from "../../lib/bridge";
import type { AppConfig } from "../../lib/types";
import {
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
  Info,
  X,
} from "lucide-react";

import logoUrl from "../../assets/logo.png";
import { Toggle, InfoRow, AboutLink, SettingsPage, SettingsCard } from "./shared";
import { KeybindGroup } from "./KeybindGroup";
import { UpdateSection } from "./UpdateSection";
import { ShellDiagnosticsBanner } from "./ShellDiagnosticsBanner";
import { AgentsOverview, AgentProviderScreen } from "./AgentsTab";
import { IntegrationsTab } from "./IntegrationsTab";
import { SecretsTab } from "./SecretsTab";
import { LogsTab } from "./LogsTab";
import { HelpPanel } from "../../help/HelpPanel";
import type { HelpSectionId } from "../../help/types";
import {
  type SectionId,
  SECTION_ICONS,
  SETTINGS_SECTIONS,
  sectionTitle,
  normalizeSection,
} from "./sections";
import { UI_PANEL } from "../shared/design";

interface SettingsPanelProps {
  config: AppConfig;
  onUpdateConfig: (config: AppConfig) => Promise<void>;
  onClose: () => void;
  initialSection?: SectionId | "notifications";
  initialHelpSection?: HelpSectionId;
}

type Route =
  | { kind: "root" }
  | { kind: "section"; id: SectionId }
  | { kind: "agent-provider"; providerId: string };

export function SettingsPanel({
  config,
  onUpdateConfig,
  onClose,
  initialSection,
  initialHelpSection,
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
    } else {
      setStack([{ kind: "root" }]);
    }
  }, [initialSection]);

  // Settings is a full-view modal — Esc closes it (matching the close button).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);

  const [localConfig, setLocalConfig] = useState<AppConfig>(config);
  const [themes, setThemes] = useState<string[]>([]);
  const [themesDir, setThemesDir] = useState<string>("");
  const [autostartEnabled, setAutostartEnabled] = useState<boolean | null>(null);

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

  // to the root grid is handled by closing — the titlebar menu is the entry point.
  const parent = stack.length >= 2 ? stack[stack.length - 2] : undefined;
  const parentSection = parent?.kind === "section" ? parent : undefined;

  return (
    <div className="absolute inset-0 flex flex-col overflow-hidden bg-[var(--ct-sidebar-bg)]" data-testid="settings-panel">
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
        {/* Header — back only for deep nav (provider → section); the section grid
            is reached from the titlebar menu, so its "back" is a close instead. */}
        <div className={`${UI_PANEL.toolbar} z-10 relative`}>
          <div className="flex min-w-0 flex-1 items-center">
            {parentSection && (
              <button
                data-testid="settings-back"
                onClick={pop}
                className="-ml-1 flex items-center gap-0.5 pr-2 text-[var(--ct-accent)] transition-opacity active:opacity-60"
                title="Back"
              >
                <ChevronLeft size={19} strokeWidth={2.4} className="shrink-0" />
                <span className="truncate text-sm">{sectionTitle(parentSection.id)}</span>
              </button>
            )}
          </div>
          <h2 className="pointer-events-none absolute left-1/2 max-w-[55%] -translate-x-1/2 truncate text-center text-sm font-semibold text-[var(--ct-fg)]" data-testid="settings-header-title">
            {headerTitle}
          </h2>
          <button
            data-testid="settings-close"
            onClick={onClose}
            title="Close settings (Esc)"
            aria-label="Close settings"
            className="-mr-1 flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-[var(--ct-muted)] transition-colors hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"
          >
            <X size={18} strokeWidth={2.2} />
          </button>
        </div>

        {/* Body */}
        <div className="flex flex-1 min-h-0 relative z-10">
          <div className="flex-1 overflow-y-auto px-5 py-5" data-testid="settings-body">
            {top.kind === "root" && (
              <div className="min-h-full flex items-center justify-center">
                <div className="w-full max-w-3xl grid grid-cols-1 sm:grid-cols-2 gap-3">
                  {SETTINGS_SECTIONS.map((s) => {
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
                      data-testid="settings-theme-select"
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
                          data-testid="settings-theme-swatch"
                          className="w-5 h-5 rounded border border-[var(--ct-border)]"
                          style={{ backgroundColor: color }}
                          title={color}
                        />
                      ))}
                    </div>
                    {themesDir && (
                      <button
                        data-testid="settings-open-themes-folder"
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
                        data-testid="settings-delete-custom-theme"
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
                      data-testid="settings-font-family-input"
                      value={localConfig.font.family}
                      onChange={(e) => update("font", { family: e.target.value })}
                      className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
                    />
                  </label>
                  <label className="block">
                    <span className="block text-xs text-[var(--ct-muted-fg)] mb-1">{`Size: ${localConfig.font.size}px`}</span>
                    <input
                      type="range"
                      data-testid="settings-font-size-input"
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
                      data-testid="settings-cursor-style-select"
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
                      data-testid="settings-scrollback-input"
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
                    ["next_tab", "Next Tab / Plan Entry"],
                    ["prev_tab", "Previous Tab / Plan Entry"],
                    ["reopen_tab", "Reopen Closed Tab"],
                  ]} config={localConfig} onUpdate={(c) => { setLocalConfig(c); onUpdateConfig(c).catch(() => {}); }} />
                </SettingsCard>
              </SettingsPage>
            )}

            {top.kind === "section" && top.id === "agents" && (
              <SettingsPage title="Agents" subtitle="General Agent, notifications, and per-provider configuration.">
                <AgentsOverview
                  config={localConfig}
                  onUpdate={(section, patch) => update(section, patch)}
                  onOpenProvider={(providerId) => push({ kind: "agent-provider", providerId })}
                  onUpdateFullConfig={(next) => {
                    setLocalConfig(next);
                    onUpdateConfig(next).catch(() => {});
                  }}
                />
              </SettingsPage>
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

            {top.kind === "section" && top.id === "guide" && (
              <div className="max-w-5xl mx-auto h-full min-h-[420px] flex flex-col" data-testid="settings-page" data-settings-page="guide">
                <p className="sr-only">Guide</p>
                <p className="text-sm text-[var(--ct-muted-fg)] mb-4 shrink-0">
                  What CodeTerm is and how to work with it.
                </p>
                <div className="flex-1 min-h-0">
                  <HelpPanel config={localConfig} initialSection={initialHelpSection} />
                </div>
              </div>
            )}

            {top.kind === "section" && top.id === "about" && (
              <SettingsPage title="Check for updates" subtitle="Version, updates, and creator.">
                <div className="text-center py-4">
                  <img src={logoUrl} alt="CodeTerm" width="64" height="64" className="rounded-xl mb-3 mx-auto" data-testid="settings-about-logo" />
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
