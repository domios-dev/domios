import { nativeBridge } from "../../lib/nativeBridge";
import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "../../lib/bridge";
import type { AppConfig, Tab, Team } from "../../lib/types";
import {
  ChevronRight,
  ExternalLink,
  Palette,
  MousePointer,
  Power,
  Keyboard,
  Info,
} from "lucide-react";
import { BackPill } from "../shared/BackPill";

import logoUrl from "../../assets/logo.png";
import { Toggle, InfoRow, AboutLink, SettingsPage, SettingsCard } from "./shared";
import { KeybindGroup } from "./KeybindGroup";
import { UpdateSection } from "./UpdateSection";
import { ShellDiagnosticsBanner } from "./ShellDiagnosticsBanner";
import { AgentsOverview, AgentProviderScreen } from "./AgentsTab";
import { ShellProfilesCard } from "./ShellProfilesCard";
import { GeneralAgentCard } from "./GeneralAgentCard";
import { NotificationsOverview } from "./NotificationsTab";
import { MessengersCard } from "./MessengersCard";
import { SettingsCardAnchor } from "./SettingsCardAnchor";
import { SecretsTab } from "./SecretsTab";
import { LogsTab } from "./LogsTab";
import { JobsView } from "../Jobs/JobsView";
import { jobsEntryFilter, type JobsWorkspace } from "../Jobs/jobsLayout";
import { HelpPanel } from "../../help/HelpPanel";
import type { HelpSectionId } from "../../help/types";
import type { GuideRequest } from "../../help/guideNavigation";
import {
  type SectionId,
  SECTION_ICONS,
  SETTINGS_SECTIONS,
  sectionTitle,
  resolveSettingsLink,
  settingsRouteForHelpTarget,
  type SettingsCardAnchorId,
  type SettingsLinkId,
} from "./sections";

interface SettingsPanelProps {
  config: AppConfig;
  onUpdateConfig: (config: AppConfig) => Promise<void>;
  onClose: () => void;
  initialSection?: SettingsLinkId;
  guideRequest?: GuideRequest;
  workspaces?: readonly JobsWorkspace[];
  tabs?: Record<string, Tab>;
  teams?: readonly Team[];
}

const NO_WORKSPACES: readonly JobsWorkspace[] = [];

function initialStack(link: SettingsLinkId | undefined, guideRequest: GuideRequest | undefined): Route[] {
  const route = resolveSettingsLink(link);
  if (!route) return [{ kind: "root" }];
  const helpSection = route.section === "guide" ? guideRequest?.section : undefined;
  return [{ kind: "root" }, { kind: "section", id: route.section, helpSection, card: route.card }];
}
const NO_TEAMS: readonly Team[] = [];

/** Names the settings dialog from its live header — one panel is mounted at a time. */
const SETTINGS_TITLE_ID = "codeterm-settings-title";

type Route =
  | { kind: "root" }
  | { kind: "section"; id: SectionId; helpSection?: HelpSectionId; card?: SettingsCardAnchorId }
  | { kind: "agent-provider"; providerId: string };

export function SettingsPanel({
  config,
  onUpdateConfig,
  onClose,
  initialSection,
  guideRequest,
  workspaces = NO_WORKSPACES,
  tabs = {},
  teams = NO_TEAMS,
}: SettingsPanelProps) {
  const [stack, setStack] = useState<Route[]>(() => initialStack(initialSection, guideRequest));
  const top = stack[stack.length - 1];
  const push = useCallback((r: Route) => setStack((s) => [...s, r]), []);
  const pop = useCallback(
    () => setStack((s) => (s.length > 1 ? s.slice(0, -1) : s)),
    [],
  );
  useEffect(() => {
    setStack(initialStack(initialSection, guideRequest));
  }, [initialSection, guideRequest]);

  // Settings is a full-view modal — Esc closes it (matching the close button).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);

  const [draft, setDraft] = useState({ config, save: onUpdateConfig });
  const localConfig = draft.save === onUpdateConfig ? draft.config : config;
  const draftRef = useRef(draft);
  draftRef.current = draft.save === onUpdateConfig ? draft : { config, save: onUpdateConfig };
  const [themes, setThemes] = useState<string[]>([]);
  const [themesDir, setThemesDir] = useState<string>("");
  const [autostartEnabled, setAutostartEnabled] = useState<boolean | null>(null);

  const builtinThemes = ["github-dark", "dracula", "catppuccin-mocha", "one-dark", "nord", "github-light", "solarized-light", "pip-boy"];

  const refreshThemes = useCallback(() => {
    invoke<string[]>("list_themes").then(setThemes).catch(() => setThemes(builtinThemes));
  }, []);

  useEffect(() => {
    setDraft({ config, save: onUpdateConfig });
  }, [config, onUpdateConfig]);

  useEffect(() => {
    refreshThemes();
    invoke<string>("get_themes_dir").then(setThemesDir).catch(() => {});
    nativeBridge.autostart?.get()
      .then(setAutostartEnabled)
      .catch(() => setAutostartEnabled(false));
  }, []);

  const handleAutostartToggle = useCallback(async (v: boolean) => {
    setAutostartEnabled(v);
    try {
      await nativeBridge.autostart?.set(v);
    } catch (e) {
      console.error("set_autostart_enabled failed", e);
      // Re-read actual state so the toggle reflects truth on failure.
      nativeBridge.autostart?.get()
        .then(setAutostartEnabled)
        .catch(() => {});
    }
  }, []);

  const saveDraft = useCallback((next: AppConfig) => {
    if (draftRef.current.save !== onUpdateConfig) return;
    const updated = { config: next, save: onUpdateConfig };
    draftRef.current = updated;
    setDraft(updated);
    void onUpdateConfig(next).catch(() => {});
  }, [onUpdateConfig]);

  const update = useCallback(
    <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => {
      const previous = draftRef.current.config;
      saveDraft({ ...previous, [section]: Object.assign({}, previous[section], patch) });
    },
    [saveDraft],
  );

  const headerTitle =
    top.kind === "root"
      ? "Settings"
      : top.kind === "section"
      ? sectionTitle(top.id)
      : "Provider";

  // to the root grid is handled by closing — the titlebar menu is the entry point.
  const jobsPage = top.kind === "section" && top.id === "jobs";
  // Jobs owns its header; opened straight onto it, Back leaves Settings instead of landing on the grid.
  const leaveJobs = stack.length <= 2 && resolveSettingsLink(initialSection)?.section === "jobs" ? onClose : pop;
  const parent = stack.length >= 2 ? stack[stack.length - 2] : undefined;
  const parentSection = parent?.kind === "section" ? parent : undefined;

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby={SETTINGS_TITLE_ID}
      className="absolute inset-0 flex flex-col overflow-hidden bg-[var(--ct-bg)]"
      data-testid="settings-panel"
    >
      <div className="absolute inset-0 flex flex-col pt-[var(--ct-safe-top,0px)] pb-[var(--ct-safe-bottom,0px)]">
        {jobsPage ? (
          <h2 id={SETTINGS_TITLE_ID} className="sr-only" data-testid="settings-header-title">{headerTitle}</h2>
        ) : (
          <div className="ct-jobs-header ct-settings-head">
            {parentSection
              ? <BackPill label={sectionTitle(parentSection.id)} onBack={pop} testId="settings-back" />
              : <BackPill ariaLabel="Close settings" onBack={onClose} testId="settings-close" />}
            <h2 id={SETTINGS_TITLE_ID} className="ct-settings-head__title" data-testid="settings-header-title">
              {headerTitle}
            </h2>
          </div>
        )}

        {/* Body */}
        <div className="flex flex-1 min-h-0 relative z-10">
          <div className="flex-1 overflow-y-auto px-5 py-5" data-testid="settings-body">
            {top.kind === "root" && (
              <div role="navigation" aria-label="Settings sections" className="ct-settings-nav">
                {SETTINGS_SECTIONS.map((s) => {
                  const Icon = SECTION_ICONS[s.id];
                  return (
                    <button
                      key={s.id}
                      type="button"
                      data-testid={`settings-section-${s.id}`}
                      onClick={() => push({ kind: "section", id: s.id })}
                      className="ct-card ct-settings-nav__item"
                    >
                      <span className="ct-settings-card__icon" aria-hidden="true"><Icon size={16} strokeWidth={2} /></span>
                      <span className="flex min-w-0 flex-col gap-0.5">
                        <span className="ct-settings-nav__label">{s.label}</span>
                        <span className="ct-settings-nav__desc">{s.desc}</span>
                      </span>
                      <ChevronRight size={14} aria-hidden="true" className="ct-settings-nav__chevron" />
                    </button>
                  );
                })}
              </div>
            )}

            {top.kind === "section" && top.id === "general" && (
              <SettingsPage title="General">
                <ShellDiagnosticsBanner onDismissSettings={onClose} />

                <SettingsCard
                  icon={<Power size={16} />}
                  title="Startup"
                  description="Resume the daemon, mesh and crons without opening the app."
                >
                  {nativeBridge.autostart && <Toggle
                    label="Launch Domios on system start"
                    checked={autostartEnabled ?? false}
                    onChange={handleAutostartToggle}
                  />}
                  <Toggle
                    label="Enable Librarian (background skill vectorizer)"
                    checked={localConfig.memory?.librarian_enabled ?? true}
                    onChange={(v) => update("memory", { librarian_enabled: v })}
                  />
                </SettingsCard>

                <SettingsCard icon={<Palette size={16} />} title="Appearance">
                  <label className="ct-settings-toggle">
                    <span>Theme</span>
                    <select
                      data-testid="settings-theme-select"
                      value={localConfig.theme.name}
                      onChange={(e) => update("theme", { name: e.target.value })}
                      className="ct-field w-56"
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
                    {themesDir && nativeBridge.window && (
                      <button
                        type="button"
                        data-testid="settings-open-themes-folder"
                        onClick={() => nativeBridge.openFolder(themesDir)}
                        className="ct-bar-pill ct-settings-link"
                        title={`Open ${themesDir}`}
                      >
                        Themes folder
                      </button>
                    )}
                    {!builtinThemes.includes(localConfig.theme.name) && (
                      <button
                        type="button"
                        data-testid="settings-delete-custom-theme"
                        onClick={async () => {
                          await invoke("delete_custom_theme", { themeId: localConfig.theme.name });
                          update("theme", { name: "github-dark" });
                          refreshThemes();
                        }}
                        className="ct-bar-pill ct-settings-link text-[var(--ct-red)]"
                      >
                        Delete custom theme
                      </button>
                    )}
                  </div>
                  <label className="ct-settings-toggle">
                    <span>Font</span>
                    <input
                      type="text"
                      data-testid="settings-font-family-input"
                      value={localConfig.font.family}
                      onChange={(e) => update("font", { family: e.target.value })}
                      className="ct-field w-56"
                    />
                  </label>
                  <label className="ct-settings-toggle">
                    <span>{`Size ${localConfig.font.size}px`}</span>
                    <input
                      type="range"
                      data-testid="settings-font-size-input"
                      min={8}
                      max={28}
                      value={localConfig.font.size}
                      onChange={(e) => update("font", { size: parseInt(e.target.value, 10) })}
                      className="w-56 accent-[var(--ct-accent)]"
                    />
                  </label>
                  <Toggle
                    label="Ligatures"
                    checked={localConfig.font.ligatures}
                    onChange={(v) => update("font", { ligatures: v })}
                  />
                </SettingsCard>

                <GeneralAgentCard config={localConfig} onUpdate={update} />

                <SettingsCardAnchor id="telegram-bridge" focused={top.card === "telegram-bridge"}>
                  <MessengersCard config={localConfig} onUpdate={update} />
                </SettingsCardAnchor>

                <ShellProfilesCard config={localConfig} onUpdate={update} />

                <SettingsCard icon={<MousePointer size={16} />} title="Terminal">
                  <label className="ct-settings-toggle">
                    <span>Cursor</span>
                    <select
                      data-testid="settings-cursor-style-select"
                      value={localConfig.terminal.cursor_style}
                      onChange={(e) => update("terminal", { cursor_style: e.target.value })}
                      className="ct-field w-56"
                    >
                      <option value="block">Block</option>
                      <option value="underline">Underline</option>
                      <option value="bar">Bar</option>
                    </select>
                  </label>
                  <Toggle
                    label="Blinking cursor"
                    checked={localConfig.terminal.cursor_blink}
                    onChange={(v) => update("terminal", { cursor_blink: v })}
                  />
                  <label className="ct-settings-toggle">
                    <span>{`Scrollback ${localConfig.terminal.scrollback.toLocaleString()} lines`}</span>
                    <input
                      type="range"
                      data-testid="settings-scrollback-input"
                      min={1000}
                      max={100000}
                      step={1000}
                      value={localConfig.terminal.scrollback}
                      onChange={(e) => update("terminal", { scrollback: parseInt(e.target.value, 10) })}
                      className="w-56 accent-[var(--ct-accent)]"
                    />
                  </label>
                  <Toggle
                    label="Copy on select"
                    checked={localConfig.terminal.copy_on_select}
                    onChange={(v) => update("terminal", { copy_on_select: v })}
                  />
                </SettingsCard>

                <SettingsCard icon={<Keyboard size={16} />} title="Keybindings">
                  <KeybindGroup title="Tabs" bindings={[
                    ["next_tab", "Next Tab / Plan Entry"],
                    ["prev_tab", "Previous Tab / Plan Entry"],
                    ["reopen_tab", "Reopen Closed Tab"],
                  ]} config={localConfig} onUpdate={saveDraft} />
                </SettingsCard>
              </SettingsPage>
            )}

            {top.kind === "section" && top.id === "notifications" && (
              <SettingsPage title="Notifications">
                <NotificationsOverview
                  config={localConfig}
                  onUpdate={(section, patch) => update(section, patch)}
                  onUpdateFullConfig={saveDraft}
                />
              </SettingsPage>
            )}

            {top.kind === "section" && top.id === "agents" && (
              <SettingsPage title="Agents">
                <AgentsOverview
                  config={localConfig}
                  onUpdate={(section, patch) => update(section, patch)}
                  onOpenProvider={(providerId) => push({ kind: "agent-provider", providerId })}
                  onOpenGuide={(helpSection) => push({ kind: "section", id: "guide", helpSection })}
                  focusCard={top.card}
                />
              </SettingsPage>
            )}

            {top.kind === "agent-provider" && (
              <div className="max-w-[720px] mx-auto">
                <AgentProviderScreen
                  providerId={top.providerId}
                  fullConfig={localConfig}
                  onUpdateFullConfig={saveDraft}
                  onProviderGone={pop}
                />
              </div>
            )}

            {top.kind === "section" && top.id === "secrets" && (
              <SettingsPage title="Secrets">
                <SecretsTab
                  config={localConfig}
                  onUpdate={(section, patch) => update(section, patch)}
                  onCloseSettings={onClose}
                />
              </SettingsPage>
            )}

            {top.kind === "section" && top.id === "jobs" && (
              <div className="-mx-5 -my-5 h-[calc(100%+2.5rem)] min-h-[420px]" data-settings-page="jobs">
                <JobsView workspaces={workspaces} teams={teams} tabs={tabs} initialFilter={jobsEntryFilter({ kind: "machine" })} onClose={leaveJobs} />
              </div>
            )}

            {top.kind === "section" && top.id === "logs" && (
              <div className="-mx-5 -my-5 h-[calc(100%+2.5rem)] min-h-[420px]" data-settings-page="logs">
                <LogsTab config={localConfig} />
              </div>
            )}

            {top.kind === "section" && top.id === "guide" && (
              <div className="max-w-5xl mx-auto h-full min-h-[420px] flex flex-col" data-testid="settings-page" data-settings-page="guide">
                <p className="sr-only">Guide</p>
                <p className="text-sm text-[var(--ct-muted-fg)] mb-4 shrink-0">
                  What Domios is and how to work with it.
                </p>
                <div className="flex-1 min-h-0">
                  <HelpPanel
                    config={localConfig}
                    initialSection={top.helpSection}
                    focusRequest={guideRequest}
                    onOpenSettings={(target) => {
                      const route = settingsRouteForHelpTarget(target);
                      push({ kind: "section", id: route.section, card: route.card });
                    }}
                  />
                </div>
              </div>
            )}

            {top.kind === "section" && top.id === "about" && (
              <SettingsPage title="Check for updates">
                <div className="text-center py-4">
                  <img src={logoUrl} alt="Domios" width="64" height="64" className="rounded-xl mb-3 mx-auto" data-testid="settings-about-logo" />
                  <div className="text-xl font-bold text-[var(--ct-fg)]">Domios</div>
                  <div className="text-xs text-[var(--ct-muted-fg)] mt-1">AI-first terminal multiplexer</div>
                </div>

                <SettingsCard icon={<Info size={16} />} title="Info">
                  <InfoRow label="Version" value={__APP_VERSION__} />
                </SettingsCard>

                <UpdateSection config={localConfig} onUpdate={update} />

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
