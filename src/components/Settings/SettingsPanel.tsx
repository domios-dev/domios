import { DropdownSelect } from "../shared/DropdownMenu";
import { useSettingsNavigation, type SettingsRoute as Route } from "../../hooks/useSettingsNavigation";
import { nativeBridge } from "../../lib/nativeBridge";
import { useState, useEffect, useLayoutEffect, useCallback, useContext, useRef } from "react";
import { invoke } from "../../lib/bridge";
import { pushBackDismissal } from "../../lib/backDismissal";
import type { AppConfig } from "../../lib/types";
import { useMuxSelector } from "../../store/multiplexer/hooks";
import { HYDRATED_GENERATION } from "../../store/multiplexer/initialState";
import type { ExtendedState } from "../../store/multiplexer/tree";
import {
  ChevronRight,
  Palette,
  MousePointer,
  Power,
  Keyboard,
} from "lucide-react";
import { BackPill } from "../shared/BackPill";
import { Loader } from "../shared/Loader";

import { Toggle, SettingsPage, SettingsCard } from "./shared";
import { KeybindGroup } from "./KeybindGroup";
import { AboutPage } from "./AboutPage";
import { ShellDiagnosticsBanner } from "./ShellDiagnosticsBanner";
import { AgentsOverview, AgentProviderScreen } from "./AgentsTab";
import { ShellProfilesCard } from "./ShellProfilesCard";
import { BrowserUploadFolders } from "./BrowserUploadFolders";
import { EmbedderCard } from "./embedder/EmbedderCard";
import { GeneralAgentCard } from "./GeneralAgentCard";
import { NotificationsOverview } from "./NotificationsTab";
import { MessengersCard } from "./MessengersCard";
import { SettingsCardAnchor } from "./SettingsCardAnchor";
import { AppearanceLabUndo } from "./AppearanceLabUndo";
import { QuickWallpaper } from "./QuickWallpaper";
import { appearanceError } from "../../lib/appearanceClient";
import { MachineAppearanceContext } from "../../lib/useMachineAppearance";
import { daemonTransportForPeer } from "../../lib/daemonBootstrap";
import { AccountTab } from "./AccountTab";
import { NetworkTab as MeshSettings } from "../MeshPanel/NetworkTab";
import { AnalyticsToggle } from "./AnalyticsToggle";
import { SecretsTab } from "./SecretsTab";
import { LogsTab } from "./LogsTab";
import { JobsView } from "../Jobs/JobsView";
import { jobsEntryFilter } from "../Jobs/jobsLayout";
import { HelpPanel } from "../../help/HelpPanel";
import type { GuideRequest } from "../../help/guideNavigation";
import {
  SECTION_ICONS,
  SETTINGS_SECTIONS,
  sectionTitle,
  settingsRouteForHelpTarget,
  type SettingsLinkId,
} from "./sections";

interface SettingsPanelProps {
  config: AppConfig;
  onUpdateConfig: (config: AppConfig) => Promise<void>;
  onClose: () => void;
  initialSection?: SettingsLinkId;
  guideRequest?: GuideRequest;
  onOpenAppearance?: (showWallpaper?: boolean) => void;
  restoreNavigation?: boolean;
}

const selectWorkspaces = (state: ExtendedState) => state.workspaces;
const selectTabs = (state: ExtendedState) => state.tabs;
const selectTeams = (state: ExtendedState) => state.teams;
const selectNavigationReady = (state: ExtendedState) => state.generation > HYDRATED_GENERATION;

function MachineJobsView({ onClose }: { onClose: () => void }) {
  const workspaces = useMuxSelector(selectWorkspaces);
  const tabs = useMuxSelector(selectTabs);
  const teams = useMuxSelector(selectTeams);
  const ready = useMuxSelector(selectNavigationReady);
  return ready ? <JobsView workspaces={workspaces} teams={teams} tabs={tabs} initialFilter={jobsEntryFilter({ kind: "machine" })} onClose={onClose} /> : <Loader label="Reading jobs…" />;
}

/** Names the settings dialog from its live header — one panel is mounted at a time. */
const SETTINGS_TITLE_ID = "codeterm-settings-title";

function routeTitle(route: Route): string {
  if (route.kind === "section") return sectionTitle(route.id);
  if (route.kind === "appearance-lab") return "Appearance Lab";
  return route.kind === "agent-provider" ? "Harness" : "Settings";
}

export function SettingsPanel({
  config,
  onUpdateConfig,
  onClose,
  initialSection,
  guideRequest,
  onOpenAppearance,
  restoreNavigation,
}: SettingsPanelProps) {
  const [stack, setStack] = useSettingsNavigation(initialSection, guideRequest, restoreNavigation);
  const top = stack[stack.length - 1];
  const parent = stack[stack.length - 2];
  const push = useCallback((r: Route) => setStack((s) => [...s, r]), [setStack]);
  const pop = useCallback(
    () => setStack((s) => (s.length > 1 ? s.slice(0, -1) : s)),
    [setStack],
  );
  const back = parent ? pop : onClose;
  const body = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (top.kind === "appearance-lab") onOpenAppearance?.(top.showOwn);
  }, [top, onOpenAppearance]);
  useEffect(() => pushBackDismissal(back), [back, top]);

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
  const machineAppearance = useContext(MachineAppearanceContext);
  const theme = machineAppearance?.appearance.theme ?? localConfig.theme;
  const [themeError, setThemeError] = useState("");
  const [themesDir, setThemesDir] = useState<string>("");
  const [autostartEnabled, setAutostartEnabled] = useState<boolean | null>(null);

  const builtinThemes = ["github-dark", "dracula", "catppuccin-mocha", "one-dark", "nord", "github-light", "solarized-light", "pip-boy"];
  const themeIds = machineAppearance?.peerId ? builtinThemes : themes;
  const chooseTheme = async (name: string) => {
    setThemeError("");
    try {
      await daemonTransportForPeer(machineAppearance?.peerId).json("POST", "/api/themes/apply", { name });
    } catch (failure) {
      setThemeError(appearanceError(failure));
    }
  };

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

  const switchGeneralProvider = useCallback(async (provider: string) => {
    const previous = draftRef.current.config;
    const next = { ...previous, general_agent: { ...previous.general_agent, agent_provider: provider } };
    await onUpdateConfig(next);
    const updated = { config: next, save: onUpdateConfig };
    draftRef.current = updated;
    setDraft(updated);
    if (next.general_agent.enabled !== false) await invoke("restart_general_agent");
  }, [onUpdateConfig]);

  const headerTitle = routeTitle(top);
  const jobsPage = top.kind === "section" && top.id === "jobs";
  const appearancePage = top.kind === "appearance-lab";

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby={SETTINGS_TITLE_ID}
      // Clip, not hidden: a clipping box is no scroll container, so no scrollIntoView inside can shift the layer.
      className="absolute inset-0 flex flex-col overflow-clip bg-[var(--ct-bg)]"
      data-appearance-surface
      data-testid="settings-panel"
    >
      <div className="absolute inset-0 flex flex-col pt-[var(--ct-safe-top,0px)] pb-[var(--ct-safe-bottom,0px)]">
        {jobsPage ? (
          <h2 id={SETTINGS_TITLE_ID} className="sr-only" data-testid="settings-header-title">{headerTitle}</h2>
        ) : (
          <div className="ct-jobs-header ct-settings-head">
            {parent
              ? <BackPill label={routeTitle(parent)} onBack={back} testId="settings-back" />
              : <BackPill ariaLabel="Close settings" onBack={back} testId="settings-close" />}
            <h2 id={SETTINGS_TITLE_ID} className="ct-settings-head__title" data-testid="settings-header-title">
              {headerTitle}
            </h2>
          </div>
        )}

        {/* Body */}
        <div className="flex flex-1 min-h-0 relative z-10">
          <div ref={body} className="flex-1 overflow-y-auto px-5 py-5" data-testid="settings-body">
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

            {top.kind === "section" && top.id === "account" && <AccountTab focusCard={top.card} />}
            {top.kind === "section" && top.id === "mesh" && <MeshSettings />}
            {appearancePage && <Loader label="Opening Appearance Lab…" />}

            {top.kind === "section" && top.id === "general" && (
              <SettingsPage title="General">
                <AnalyticsToggle />
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

                <GeneralAgentCard config={localConfig} onUpdate={update} onSwitchProvider={switchGeneralProvider} onOpenGuide={(helpSection) => push({ kind: "section", id: "guide", helpSection })} />

                <SettingsCard icon={<Palette size={16} />} title="Appearance" description="Theme and wallpaper for this machine"
                  headerRight={onOpenAppearance && <button type="button" className="ct-settings-link" onClick={() => onOpenAppearance()}>Open Appearance Lab ›</button>}>
                  {onOpenAppearance && <AppearanceLabUndo key={machineAppearance?.peerId ?? "local"} peer={machineAppearance?.peerId} />}
                  <label className="ct-settings-toggle">
                    <span>Theme</span>
                    <div className="w-56 min-w-0">
                      <DropdownSelect
                        data-testid="settings-theme-select"
                        aria-label="Theme"
                        value={theme.name}
                        onChange={(name) => void chooseTheme(name)}
                        options={[...new Set([...themeIds, theme.name])].map((name) => ({
                          value: name,
                          label: name,
                          group: builtinThemes.includes(name) ? "Built-in" : themeIds.includes(name) ? "Custom" : "Current look",
                        }))}
                      />
                    </div>
                  </label>
                  <div className="flex items-center gap-2">
                    <div className="flex gap-1 flex-1">
                      {[
                        theme.background,
                        theme.foreground,
                        theme.red,
                        theme.green,
                        theme.yellow,
                        theme.blue,
                        theme.magenta,
                        theme.cyan,
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
                    {themesDir && nativeBridge.window && !machineAppearance?.peerId && (
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
                    {!machineAppearance?.peerId && themes.includes(theme.name) && !builtinThemes.includes(theme.name) && (
                      <button
                        type="button"
                        data-testid="settings-delete-custom-theme"
                        onClick={async () => {
                          await invoke("delete_custom_theme", { themeId: theme.name });
                          await chooseTheme("github-dark");
                          refreshThemes();
                        }}
                        className="ct-bar-pill ct-settings-link text-[var(--ct-red)]"
                      >
                        Delete custom theme
                      </button>
                    )}
                  </div>
                  {themeError && <p role="alert" className="ct-appearance-error">{themeError}</p>}
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
                  <QuickWallpaper config={localConfig} onOpenLab={onOpenAppearance ? () => onOpenAppearance(true) : undefined} />
                </SettingsCard>

                <SettingsCardAnchor id="telegram-bridge" focused={top.card === "telegram-bridge"}>
                  <MessengersCard config={localConfig} onUpdate={update} />
                </SettingsCardAnchor>

                <ShellProfilesCard config={localConfig} onUpdate={update} />
                <BrowserUploadFolders config={localConfig} onSave={async next => {
                  await onUpdateConfig(next);
                  const updated = { config: next, save: onUpdateConfig };
                  draftRef.current = updated;
                  setDraft(updated);
                }} />
                <EmbedderCard config={localConfig} />

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
                    ["new_tab", "New Tab"],
                    ["close_tab", "Close Tab"],
                    ["next_tab", "Next Tab / Plan Entry"],
                    ["prev_tab", "Previous Tab / Plan Entry"],
                    ["reopen_tab", "Reopen Closed Tab"],
                    ["new_workspace", "New Workspace"],
                  ]} config={localConfig} onUpdate={saveDraft} />
                  <KeybindGroup title="Terminal" bindings={[
                    ["copy", "Copy Selection"],
                    ["paste", "Paste"],
                    ["search", "Find"],
                  ]} config={localConfig} onUpdate={saveDraft} />
                  <KeybindGroup title="View" bindings={[
                    ["zoom_in", "Zoom In"],
                    ["zoom_out", "Zoom Out"],
                    ["zoom_reset", "Reset Zoom"],
                    ["open_settings", "Settings"],
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
                  onSwitchGeneralProvider={switchGeneralProvider}
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
                  focusCard={top.card}
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
                <MachineJobsView onClose={back} />
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

            {top.kind === "section" && top.id === "about" && <AboutPage config={localConfig} onUpdate={update} />}
          </div>
        </div>

      </div>
    </div>
  );
}
