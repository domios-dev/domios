import { useState, useEffect, useCallback, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AppConfig, ProviderConfig } from "../../lib/types";
import { MeshSection } from "./MeshSection";
import { CloseXIcon, CheckIcon, CopyIcon } from "../Icons";
import { isMac } from "../../lib/keybindings";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { SchemaRenderer } from "./SchemaRenderer";
import { useUpdate } from "../../hooks/useUpdate";
import type { UpdateChannel } from "../../hooks/useUpdate";
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
                <SecretsTab config={localConfig} onUpdate={(section, patch) => update(section, patch)} />
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

// ---------------------------------------------------------------------------
// Sub-components
// ---------------------------------------------------------------------------

const CHANNELS: { value: UpdateChannel; label: string; hint: string }[] = [
  { value: "stable", label: "Stable", hint: "Production releases" },
  { value: "rc", label: "RC", hint: "Release candidates" },
  { value: "nightly", label: "Nightly", hint: "Bleeding edge" },
];

function UpdateSection() {
  const {
    status,
    currentVersion,
    latestVersion,
    releaseNotes,
    releaseUrl,
    channel,
    channelSupported,
    setChannel,
    refresh,
    applyUpdate,
    applying,
    error,
  } = useUpdate();

  const updateAvailable = status === "available" && latestVersion && latestVersion !== currentVersion;

  return (
    <Group title="Updates">
      <div className="space-y-3">
        <div className="flex items-center gap-3 flex-wrap">
          <button
            onClick={() => void refresh(true)}
            disabled={applying || status === "checking"}
            className="px-3 py-1.5 text-[11px] rounded bg-[var(--ct-border)] text-[var(--ct-fg)] hover:bg-[var(--ct-muted)] transition-colors disabled:opacity-50"
          >
            {status === "checking" ? "Checking..." : "Check for Updates"}
          </button>
          {status === "up-to-date" && (
            <span className="text-[11px] text-[var(--ct-green)]">Up to date</span>
          )}
          {updateAvailable && (
            <>
              <span className="text-[11px] text-[var(--ct-accent)]">v{latestVersion} available</span>
              <button
                onClick={() => void applyUpdate()}
                disabled={applying}
                className="px-3 py-1.5 text-[11px] rounded bg-[var(--ct-accent)] text-white hover:opacity-90 transition-opacity font-medium disabled:opacity-50"
              >
                {applying ? "Installing..." : "Update now"}
              </button>
            </>
          )}
          {error && (
            <span className="text-[11px] text-[var(--ct-red)]">{error}</span>
          )}
        </div>

        {applying && (
          <div className="p-2 rounded bg-[var(--ct-accent)]/10 border border-[var(--ct-accent)]/30 text-[var(--ct-accent)] text-[11px]">
            Downloading and installing... The app will restart automatically.
          </div>
        )}

        {channelSupported && (
          <div>
            <label className="text-[10px] uppercase tracking-wider text-[var(--ct-muted)] block mb-1.5">
              Release channel
            </label>
            <div className="flex gap-1.5">
              {CHANNELS.map((c) => {
                const active = channel === c.value;
                return (
                  <button
                    key={c.value}
                    onClick={() => void setChannel(c.value)}
                    disabled={applying || channel === null}
                    title={c.hint}
                    className={
                      "flex-1 px-2.5 py-1.5 rounded border text-[11px] transition-colors disabled:opacity-50 " +
                      (active
                        ? "bg-[var(--ct-accent)] text-white border-[var(--ct-accent)]"
                        : "bg-transparent text-[var(--ct-fg)] border-[var(--ct-border)] hover:bg-[var(--ct-hover)]")
                    }
                  >
                    {c.label}
                  </button>
                );
              })}
            </div>
          </div>
        )}

        {updateAvailable && releaseNotes && releaseNotes.trim().length > 0 && (
          <div>
            <div className="flex items-center justify-between mb-1.5">
              <label className="text-[10px] uppercase tracking-wider text-[var(--ct-muted)]">
                Release notes
              </label>
              {releaseUrl && (
                <a
                  href={releaseUrl}
                  target="_blank"
                  rel="noreferrer"
                  className="text-[10px] text-[var(--ct-accent)] hover:underline"
                >
                  Open on GitHub
                </a>
              )}
            </div>
            <div
              className="bg-[var(--ct-surface)] border border-[var(--ct-border)] rounded p-3 text-[11px] text-[var(--ct-fg)] font-mono leading-relaxed whitespace-pre-wrap"
              style={{ maxHeight: 200, overflowY: "auto" }}
            >
              {releaseNotes}
            </div>
          </div>
        )}
      </div>
    </Group>
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

function AboutLink({ href, label, Icon }: { href: string; label: string; Icon: LucideIcon }) {
  return (
    <a
      href={href}
      target="_blank"
      rel="noopener noreferrer"
      className="inline-flex items-center gap-1.5 text-xs text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] transition-colors"
    >
      <Icon size={14} className="shrink-0" />
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

function AgentsOverview({
  config,
  onUpdate,
  onOpenProvider,
}: {
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
  onOpenProvider: (providerId: string) => void;
}) {
  const { providers, loading, error } = useAgentProviders();

  return (
    <div className="space-y-5">
      {/* General Agent — shared across all providers */}
      <div className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4 space-y-3">
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
          Skills are a CodeTerm feature that syncs to all providers. */}
      <div className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4 space-y-3">
        <div className="flex items-center justify-between">
          <div className="text-sm font-medium text-[var(--ct-fg)]">Skills</div>
          <span className="text-[9px] px-1.5 py-0.5 rounded-full bg-[var(--ct-green)]/20 text-[var(--ct-green)]">
            auto-synced
          </span>
        </div>
        <div className="text-xs text-[var(--ct-muted-fg)]">
          Drop skill folders into <code className="text-[10px]">~/.codeterm/skills/</code> — they're
          available to every agent provider automatically.
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

      {/* Providers — tile list, click to open detail screen */}
      <div>
        <div className="text-[10px] font-medium uppercase tracking-[0.06em] text-[var(--ct-muted)] mb-2.5">Providers</div>
        {error && (
          <div className="text-[10px] text-[var(--ct-red)] mb-3">
            Failed to load agent providers: {error}
          </div>
        )}
        {loading && providers.length === 0 && (
          <div className="text-[10px] text-[var(--ct-muted-fg)]">Loading providers…</div>
        )}
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
          {providers.map((p) => (
            <button
              key={p.id}
              onClick={() => onOpenProvider(p.id)}
              className="flex items-center gap-3 p-3.5 rounded-xl border border-[var(--ct-border)] bg-[var(--ct-bg)]/40 hover:bg-[var(--ct-hover)] hover:border-[var(--ct-accent)]/50 transition-colors text-left"
            >
              <div className="w-9 h-9 rounded-lg flex items-center justify-center bg-[var(--ct-accent)]/15 text-[var(--ct-accent)] text-base shrink-0">
                {p.icon_hint || p.display_name.slice(0, 1).toUpperCase()}
              </div>
              <div className="min-w-0 flex-1">
                <div className="text-[13px] font-medium text-[var(--ct-fg)]">{p.display_name}</div>
                <div className="text-[11px] text-[var(--ct-muted-fg)] mt-0.5">{p.id}</div>
              </div>
              <ChevronRight size={14} className="shrink-0 text-[var(--ct-muted)]" />
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

function AgentProviderScreen({
  providerId,
  fullConfig,
  onUpdateFullConfig,
  onProviderGone,
}: {
  providerId: string;
  fullConfig: AppConfig;
  onUpdateFullConfig: (next: AppConfig) => void;
  onProviderGone: () => void;
}) {
  const { providers, loading, error } = useAgentProviders();
  const provider = providers.find((p) => p.id === providerId) ?? null;

  // If a plugin was deactivated mid-navigation, pop back to the Agents
  // overview so the user isn't stuck on a dead provider detail.
  useEffect(() => {
    if (!loading && !provider) onProviderGone();
  }, [loading, provider, onProviderGone]);

  const onProviderConfigChange = useCallback(
    (pid: string, patch: Partial<ProviderConfig>) => {
      const prev = fullConfig.agents?.[pid] ?? {};
      const nextProvider: ProviderConfig = { ...prev, ...patch };
      const nextAgents = { ...(fullConfig.agents ?? {}), [pid]: nextProvider };
      onUpdateFullConfig({ ...fullConfig, agents: nextAgents });
    },
    [fullConfig, onUpdateFullConfig],
  );

  if (error) {
    return <div className="text-[10px] text-[var(--ct-red)]">Failed to load agent providers: {error}</div>;
  }
  if (loading && !provider) {
    return <div className="text-[10px] text-[var(--ct-muted-fg)]">Loading provider…</div>;
  }
  if (!provider) {
    // Transient: the pop effect above will unmount us on the next tick.
    return null;
  }

  return (
    <div className="space-y-5">
      <div className="flex items-center gap-3 pb-2">
        <div className="w-10 h-10 rounded-lg flex items-center justify-center bg-[var(--ct-accent)]/15 text-[var(--ct-accent)] text-lg shrink-0">
          {provider.icon_hint || provider.display_name.slice(0, 1).toUpperCase()}
        </div>
        <div className="min-w-0">
          <div className="text-sm font-medium text-[var(--ct-fg)]">{provider.display_name}</div>
          <div className="text-[11px] text-[var(--ct-muted-fg)]">{provider.id}</div>
        </div>
      </div>
      <SchemaRenderer
        schema={provider.settings_schema}
        providerId={provider.id}
        config={fullConfig}
        onChange={onProviderConfigChange}
      />
    </div>
  );
}

function MemoryTab({ config, onUpdate }: { config: AppConfig; onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void }) {
  const defaultMem = { extraction_agent: "", extraction_model: "haiku", auto_extract: true, dreaming_enabled: true, dreaming_agent: "", dreaming_model: "haiku", worker_model: "claude-haiku-4-5-20251001", worker_timeout_seconds: 150, file_sync_enabled: true, file_sync_interval_minutes: 60, dedup_threshold: 0.15 };
  const mem = { ...defaultMem, ...(config.memory ?? {}) };
  const { providers } = useAgentProviders();
  const inputClass = "w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none";

  const defaultProvider = config.general_agent?.agent_provider || "claude";

  const extractionProvider = mem.extraction_agent || defaultProvider;
  const extractionModels = providers.find(p => p.id === extractionProvider)?.available_models ?? [];

  const dreamingProvider = mem.dreaming_agent || defaultProvider;
  const dreamingModels = providers.find(p => p.id === dreamingProvider)?.available_models ?? [];

  const [syncStatus, setSyncStatus] = useState<{ tracked_files: number; stale_files: number } | null>(null);
  const [syncing, setSyncing] = useState(false);

  const api = config.api;
  const apiBase = api ? `http://${api.host === "0.0.0.0" ? "127.0.0.1" : api.host}:${api.port}` : "";
  const apiHeaders: HeadersInit = api?.token ? { Authorization: `Bearer ${api.token}` } : {};

  useEffect(() => {
    if (!apiBase) return;
    fetch(`${apiBase}/api/mem/sync/status`, { headers: apiHeaders })
      .then(r => r.json())
      .then(d => setSyncStatus(d))
      .catch(() => {});
  }, [apiBase]);

  const triggerSync = useCallback(async () => {
    if (!apiBase) return;
    setSyncing(true);
    try {
      const r = await fetch(`${apiBase}/api/mem/sync`, { method: "POST", headers: { "Content-Type": "application/json", ...apiHeaders }, body: "{}" });
      const d = await r.json();
      setSyncStatus({ tracked_files: d.total_tracked ?? 0, stale_files: d.source_missing ?? 0 });
    } catch { /* ignore */ }
    setSyncing(false);
  }, [apiBase]);

  const triggerDream = useCallback(async () => {
    if (!apiBase) return;
    fetch(`${apiBase}/api/mem/dream`, { method: "POST", headers: { "Content-Type": "application/json", ...apiHeaders }, body: "{}" }).catch(() => {});
  }, [apiBase]);

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
        </Label>
        <Label text="Dedup threshold">
          <input
            type="number"
            min={0.05}
            max={0.5}
            step={0.01}
            value={mem.dedup_threshold}
            onChange={(e) => {
              const n = Number(e.target.value);
              if (Number.isFinite(n) && n > 0) {
                onUpdate("memory", { ...mem, dedup_threshold: n });
              }
            }}
            className={inputClass}
          />
          <p className="text-[10px] text-[var(--ct-muted)] mt-1">
            Cosine distance below this triggers dedup investigation. Lower = stricter. Default 0.15.
          </p>
        </Label>
        <Label text="Run timeout (seconds)">
          <input
            type="number"
            min={10}
            max={3600}
            value={mem.worker_timeout_seconds}
            onChange={(e) => {
              const n = Number(e.target.value);
              if (Number.isFinite(n) && n > 0) {
                onUpdate("memory", { ...mem, worker_timeout_seconds: n });
              }
            }}
            className={inputClass}
          />
        </Label>
      </Group>

      <Group title="Periodic Tasks">
        <div className="space-y-4">
          <div className="border border-[var(--ct-border)] rounded p-3">
            <div className="flex items-center justify-between mb-2">
              <span className="text-xs font-medium text-[var(--ct-fg)]">Dreaming (Nightly Optimizer)</span>
              <button onClick={triggerDream} className="text-[10px] px-2 py-0.5 rounded bg-[var(--ct-accent)] text-white hover:opacity-80">Run Now</button>
            </div>
            <p className="text-[10px] text-[var(--ct-muted)] mb-2">
              Uses LLM — consolidates, deduplicates, promotes learnings, cleans stale graph relations. Schedule: ~3 AM with jitter.
            </p>
            <Label text="Enable">
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
          </div>

          <div className="border border-[var(--ct-border)] rounded p-3">
            <div className="flex items-center justify-between mb-2">
              <span className="text-xs font-medium text-[var(--ct-fg)]">File Sync (Librarian)</span>
              <button onClick={triggerSync} disabled={syncing} className="text-[10px] px-2 py-0.5 rounded bg-[var(--ct-accent)] text-white hover:opacity-80 disabled:opacity-50">
                {syncing ? "Syncing..." : "Sync Now"}
              </button>
            </div>
            <p className="text-[10px] text-[var(--ct-muted)] mb-2">
              No LLM — pure Rust mtime comparison + re-vectorization for source-tracked files.
            </p>
            {syncStatus && (
              <p className="text-[10px] text-[var(--ct-muted)] mb-2">
                Tracked files: {syncStatus.tracked_files}{syncStatus.stale_files > 0 ? ` (${syncStatus.stale_files} stale)` : ""}
              </p>
            )}
            <Label text="Enable">
              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="checkbox"
                  checked={mem.file_sync_enabled}
                  onChange={(e) => onUpdate("memory", { ...mem, file_sync_enabled: e.target.checked })}
                  className="w-4 h-4 rounded"
                />
                <span className="text-xs text-[var(--ct-fg)]">Periodic file sync</span>
              </label>
            </Label>
            <Label text="Sync interval (minutes)">
              <input
                type="number"
                min={5}
                max={1440}
                value={mem.file_sync_interval_minutes}
                onChange={(e) => {
                  const n = Number(e.target.value);
                  if (Number.isFinite(n) && n >= 5) {
                    onUpdate("memory", { ...mem, file_sync_interval_minutes: n });
                  }
                }}
                className={inputClass}
              />
            </Label>
          </div>
        </div>
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

type SecretsBackend = "local" | "bitwarden" | "mock";

const BACKEND_LABELS: Record<SecretsBackend, string> = {
  local: "Local secret storage",
  bitwarden: "Bitwarden",
  mock: "Mock (testing only)",
};

interface SecretsStatus {
  backend: string;
  status: {
    status: "logged_out" | "locked" | "unlocked" | "unavailable";
    user?: string | null;
    transient?: boolean;
    endpoint?: string | null;
    reason?: string;
  };
}

interface SecretListItem {
  item_id: string;
  name: string;
  folder?: string | null;
  organization?: string | null;
  collection?: string | null;
  last_modified?: number | null;
}

function SecretsTab({
  config,
  onUpdate,
}: {
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
}) {
  const api = config.api;
  const apiBase = api ? `http://${api.host === "0.0.0.0" ? "127.0.0.1" : api.host}:${api.port}` : "";
  const apiHeaders: HeadersInit = api?.token
    ? { Authorization: `Bearer ${api.token}` }
    : {};
  const jsonHeaders: HeadersInit = { "Content-Type": "application/json", ...apiHeaders };

  const cfg = config.integrations?.secrets ?? {
    backend: "local",
    default_organization: "",
    default_collection: "",
    bitwarden: { server_url: "https://vault.bitwarden.com" },
  };
  const backend = (cfg.backend || "local") as SecretsBackend;
  const bitwardenServerUrl =
    (cfg as { bitwarden?: { server_url?: string } }).bitwarden?.server_url ||
    "https://vault.bitwarden.com";

  const [status, setStatus] = useState<SecretsStatus | null>(null);
  const [statusBusy, setStatusBusy] = useState(false);
  const [items, setItems] = useState<SecretListItem[]>([]);
  const [orgs, setOrgs] = useState<{ id: string; name: string }[]>([]);
  const [colls, setColls] = useState<{ id: string; name: string }[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<"password" | "apikey">("password");
  const [toast, setToast] = useState<string | null>(null);
  const [serverUrlInput, setServerUrlInput] = useState(bitwardenServerUrl);

  // Login form state — never persisted in component state past submission.
  const [email, setEmail] = useState("");
  const [masterPassword, setMasterPassword] = useState("");
  const [twoFactor, setTwoFactor] = useState("");
  const [apiClientId, setApiClientId] = useState("");
  const [apiClientSecret, setApiClientSecret] = useState("");

  const refreshStatus = useCallback(async () => {
    if (!apiBase) return;
    setStatusBusy(true);
    try {
      const r = await fetch(`${apiBase}/api/secrets/status`, { headers: apiHeaders });
      if (r.ok) setStatus(await r.json());
    } catch (e) {
      console.warn("secrets status failed:", e);
    } finally {
      setStatusBusy(false);
    }
  }, [apiBase]);

  const refreshItems = useCallback(async () => {
    if (!apiBase) return;
    try {
      const r = await fetch(`${apiBase}/api/secrets/items?limit=10`, { headers: apiHeaders });
      if (r.ok) {
        const d = await r.json();
        setItems(d.items ?? []);
      } else {
        setItems([]);
      }
    } catch {
      setItems([]);
    }
  }, [apiBase]);

  const refreshOrgs = useCallback(async () => {
    if (!apiBase) return;
    try {
      const r = await fetch(`${apiBase}/api/secrets/organizations`, { headers: apiHeaders });
      if (r.ok) {
        const d = await r.json();
        setOrgs(d.organizations ?? []);
      }
    } catch {
      // ignore
    }
  }, [apiBase]);

  const refreshColls = useCallback(
    async (orgId: string) => {
      if (!apiBase) return;
      const url = orgId
        ? `${apiBase}/api/secrets/collections?organization=${encodeURIComponent(orgId)}`
        : `${apiBase}/api/secrets/collections`;
      try {
        const r = await fetch(url, { headers: apiHeaders });
        if (r.ok) {
          const d = await r.json();
          setColls(d.collections ?? []);
        }
      } catch {
        // ignore
      }
    },
    [apiBase],
  );

  useEffect(() => {
    refreshStatus();
  }, [refreshStatus, backend]);

  useEffect(() => {
    setServerUrlInput(bitwardenServerUrl);
  }, [bitwardenServerUrl]);

  useEffect(() => {
    if (status?.status?.status === "unlocked") {
      refreshItems();
      refreshOrgs();
      refreshColls(cfg.default_organization);
    } else {
      setItems([]);
      setOrgs([]);
      setColls([]);
    }
  }, [status?.status?.status, refreshItems, refreshOrgs, refreshColls, cfg.default_organization]);

  const setBackend = useCallback(
    async (next: SecretsBackend) => {
      setError(null);
      // Optimistically update the local config so the dropdown reflects
      // the new choice; the swap endpoint persists it for real on disk.
      onUpdate("integrations", {
        secrets: {
          backend: next,
          default_organization: cfg.default_organization,
          default_collection: cfg.default_collection,
          bitwarden: { server_url: bitwardenServerUrl },
        },
      });
      // Drop stale UI state from the previous backend before the swap.
      setItems([]);
      setOrgs([]);
      setColls([]);
      setStatus(null);
      try {
        const r = await fetch(`${apiBase}/api/secrets/backend`, {
          method: "POST",
          headers: jsonHeaders,
          body: JSON.stringify({
            backend: next,
            bitwarden_server_url: next === "bitwarden" ? bitwardenServerUrl : undefined,
          }),
        });
        if (!r.ok) {
          const d = await r.json().catch(() => ({}));
          setError(d?.error?.message || `switch failed (HTTP ${r.status})`);
          return;
        }
        const d = await r.json();
        setStatus({ backend: d.backend, status: d.status });
        setToast(`Switched to ${BACKEND_LABELS[next]}`);
        setTimeout(() => setToast(null), 2500);
      } catch (e) {
        setError(String(e));
      }
    },
    [
      apiBase,
      jsonHeaders,
      onUpdate,
      cfg.default_organization,
      cfg.default_collection,
      bitwardenServerUrl,
    ],
  );

  const saveServerUrl = useCallback(async () => {
    setError(null);
    const url = serverUrlInput.trim();
    if (!/^https?:\/\/[^\s/$.?#].[^\s]*$/i.test(url)) {
      setError("Server URL must start with http:// or https://");
      return;
    }
    onUpdate("integrations", {
      secrets: {
        backend: cfg.backend,
        default_organization: cfg.default_organization,
        default_collection: cfg.default_collection,
        bitwarden: { server_url: url },
      },
    });
    // Re-trigger the backend swap so the running daemon picks up the new
    // server URL. The backend itself decides whether to force a re-login.
    try {
      const r = await fetch(`${apiBase}/api/secrets/backend`, {
        method: "POST",
        headers: jsonHeaders,
        body: JSON.stringify({ backend: "bitwarden", bitwarden_server_url: url }),
      });
      if (!r.ok) {
        const d = await r.json().catch(() => ({}));
        setError(d?.error?.message || `apply failed (HTTP ${r.status})`);
        return;
      }
      const d = await r.json();
      setStatus({ backend: d.backend, status: d.status });
      setToast(`Server URL applied — sign in again`);
      setTimeout(() => setToast(null), 2500);
    } catch (e) {
      setError(String(e));
    }
  }, [
    apiBase,
    jsonHeaders,
    onUpdate,
    cfg.backend,
    cfg.default_organization,
    cfg.default_collection,
    serverUrlInput,
  ]);

  const setDefaultOrg = useCallback(
    (orgId: string) => {
      onUpdate("integrations", {
        secrets: {
          backend: cfg.backend,
          default_organization: orgId,
          default_collection: "", // reset collection when org changes
        },
      });
    },
    [onUpdate, cfg.backend],
  );

  const setDefaultColl = useCallback(
    (collId: string) => {
      onUpdate("integrations", {
        secrets: {
          backend: cfg.backend,
          default_organization: cfg.default_organization,
          default_collection: collId,
        },
      });
    },
    [onUpdate, cfg.backend, cfg.default_organization],
  );

  const submitPasswordLogin = useCallback(
    async (e: React.FormEvent) => {
      e.preventDefault();
      setError(null);
      try {
        const r = await fetch(`${apiBase}/api/secrets/unlock`, {
          method: "POST",
          headers: jsonHeaders,
          body: JSON.stringify({
            master_password: masterPassword,
            email: email || undefined,
            two_factor_token: twoFactor || undefined,
          }),
        });
        if (!r.ok) {
          const d = await r.json().catch(() => ({}));
          setError(d?.error?.message || `unlock failed (HTTP ${r.status})`);
          return;
        }
        // Wipe form state — password lifetime ends here.
        setMasterPassword("");
        setTwoFactor("");
        await refreshStatus();
      } catch (err) {
        setError(String(err));
      }
    },
    [apiBase, jsonHeaders, masterPassword, email, twoFactor, refreshStatus],
  );

  const submitApiKeyLogin = useCallback(
    async (e: React.FormEvent) => {
      e.preventDefault();
      setError(null);
      try {
        const r = await fetch(`${apiBase}/api/secrets/unlock`, {
          method: "POST",
          headers: jsonHeaders,
          body: JSON.stringify({
            master_password: masterPassword,
            api_key_client_id: apiClientId,
            api_key_client_secret: apiClientSecret,
          }),
        });
        if (!r.ok) {
          const d = await r.json().catch(() => ({}));
          setError(d?.error?.message || `unlock failed (HTTP ${r.status})`);
          return;
        }
        setMasterPassword("");
        setApiClientSecret("");
        await refreshStatus();
      } catch (err) {
        setError(String(err));
      }
    },
    [apiBase, jsonHeaders, masterPassword, apiClientId, apiClientSecret, refreshStatus],
  );

  const lock = useCallback(async () => {
    setError(null);
    await fetch(`${apiBase}/api/secrets/lock`, { method: "POST", headers: jsonHeaders, body: "{}" });
    await refreshStatus();
  }, [apiBase, jsonHeaders, refreshStatus]);

  const logout = useCallback(async () => {
    if (!confirm("Sign out and delete the stored session token from your OS keychain?")) return;
    setError(null);
    await fetch(`${apiBase}/api/secrets/logout`, { method: "POST", headers: jsonHeaders, body: "{}" });
    await refreshStatus();
  }, [apiBase, jsonHeaders, refreshStatus]);

  const statusBadge = useMemo(() => {
    if (!status) return null;
    const s = status.status;
    switch (s.status) {
      case "unlocked":
        return (
          <span
            className="text-[10px] px-2 py-0.5 rounded-full bg-[var(--ct-green)]/20 text-[var(--ct-green)]"
            title={s.endpoint || ""}
          >
            unlocked{s.user ? ` as ${s.user}` : ""}{s.transient ? " (transient)" : ""}
          </span>
        );
      case "locked":
        return (
          <span className="text-[10px] px-2 py-0.5 rounded-full bg-[var(--ct-yellow)]/20 text-[var(--ct-yellow)]">
            locked
          </span>
        );
      case "logged_out":
        return (
          <span className="text-[10px] px-2 py-0.5 rounded-full bg-[var(--ct-border)] text-[var(--ct-muted-fg)]">
            logged out
          </span>
        );
      case "unavailable":
        return (
          <span className="text-[10px] px-2 py-0.5 rounded-full bg-[var(--ct-red)]/20 text-[var(--ct-red)]">
            unavailable
          </span>
        );
    }
  }, [status]);

  const inputClass =
    "w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none";

  return (
    <div className="space-y-5">
      <div className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4 space-y-4">
        <div className="flex items-center justify-between">
          <div className="text-sm font-medium text-[var(--ct-fg)]">Secret store</div>
          <div className="flex items-center gap-2">
            {statusBadge}
            <button
              onClick={refreshStatus}
              disabled={statusBusy}
              className="text-[10px] px-2 py-0.5 rounded border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] disabled:opacity-50"
            >
              refresh
            </button>
          </div>
        </div>
        <div className="text-xs text-[var(--ct-muted-fg)]">
          Pluggable backend for the <code className="font-mono">codeterm mem secret</code> command family.
          Secrets do not cross the mesh — only the local machine's vault is reachable.
        </div>

        <div className="space-y-1">
          <div className="text-[10px] uppercase tracking-wider text-[var(--ct-muted)]">Backend</div>
          <select
            value={backend}
            onChange={(e) => void setBackend(e.target.value as SecretsBackend)}
            className={inputClass}
          >
            <option value="local">Local secret storage</option>
            <option value="bitwarden">Bitwarden</option>
            <option value="mock">Mock (testing only)</option>
          </select>
          <div className="text-[10px] text-[var(--ct-muted-fg)]">
            {backend === "local"
              ? "Persisted in the OS keychain (macOS Keychain / libsecret / Windows Credential Manager). Offline, no external service."
              : backend === "bitwarden"
                ? "Vault accessed via the bw CLI. Supports vault.bitwarden.com and self-hosted Vaultwarden."
                : "In-memory only. Data is wiped on daemon restart — for tests and local dev."}
          </div>
        </div>

        {backend === "bitwarden" && (
          <div className="space-y-1">
            <div className="text-[10px] uppercase tracking-wider text-[var(--ct-muted)]">
              Server URL
            </div>
            <div className="flex gap-2">
              <input
                type="text"
                value={serverUrlInput}
                onChange={(e) => setServerUrlInput(e.target.value)}
                placeholder="https://vault.bitwarden.com"
                className={inputClass}
                spellCheck={false}
              />
              <button
                onClick={() => void saveServerUrl()}
                disabled={serverUrlInput.trim() === bitwardenServerUrl}
                className="text-xs px-3 py-1.5 rounded border border-[var(--ct-border)] text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] disabled:opacity-40 disabled:cursor-not-allowed"
              >
                Apply
              </button>
            </div>
            <div className="text-[10px] text-[var(--ct-muted-fg)]">
              Self-hosted Vaultwarden? Point to{" "}
              <code className="font-mono">https://your-host.example</code>. Changing the URL forces a re-login.
            </div>
          </div>
        )}

        {toast && (
          <div className="text-[11px] rounded border border-[var(--ct-green)]/40 bg-[var(--ct-green)]/10 p-2 text-[var(--ct-green)]">
            {toast}
          </div>
        )}

        {status?.status?.status === "unavailable" && (
          <div className="text-[11px] rounded border border-[var(--ct-red)]/40 bg-[var(--ct-red)]/10 p-3 text-[var(--ct-red)]">
            <div className="font-medium mb-1">Backend unavailable</div>
            <div className="text-[var(--ct-fg)]">{status.status.reason}</div>
            {backend === "bitwarden" && (
              <div className="mt-2 space-y-0.5 text-[var(--ct-muted-fg)]">
                <div>Install the Bitwarden CLI:</div>
                <div className="font-mono">macOS: <code>brew install bitwarden-cli</code></div>
                <div className="font-mono">Linux: <code>npm i -g @bitwarden/cli</code></div>
                <div className="font-mono">Windows: <code>scoop install bitwarden-cli</code></div>
              </div>
            )}
          </div>
        )}

        {error && (
          <div className="text-[11px] rounded border border-[var(--ct-red)]/40 bg-[var(--ct-red)]/10 p-2 text-[var(--ct-red)]">
            {error}
          </div>
        )}
      </div>

      {/* Login forms — only when logged out. */}
      {status?.status?.status === "logged_out" && backend === "bitwarden" && (
        <div className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4 space-y-3">
          <div className="text-sm font-medium text-[var(--ct-fg)]">Sign in to Bitwarden</div>
          <div className="flex gap-1 text-[11px]">
            <button
              onClick={() => setActiveTab("password")}
              className={`px-2.5 py-1 rounded border ${activeTab === "password" ? "bg-[var(--ct-accent)] text-[var(--ct-bg)] border-[var(--ct-accent)]" : "bg-transparent text-[var(--ct-muted-fg)] border-[var(--ct-border)]"}`}
            >
              Password
            </button>
            <button
              onClick={() => setActiveTab("apikey")}
              className={`px-2.5 py-1 rounded border ${activeTab === "apikey" ? "bg-[var(--ct-accent)] text-[var(--ct-bg)] border-[var(--ct-accent)]" : "bg-transparent text-[var(--ct-muted-fg)] border-[var(--ct-border)]"}`}
            >
              API key
            </button>
          </div>
          {activeTab === "password" ? (
            <form onSubmit={submitPasswordLogin} className="space-y-2">
              <input
                type="email"
                placeholder="email"
                autoComplete="username"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                className={inputClass}
                required
              />
              <input
                type="password"
                placeholder="master password (never persisted)"
                autoComplete="current-password"
                value={masterPassword}
                onChange={(e) => setMasterPassword(e.target.value)}
                className={inputClass}
                required
              />
              <input
                type="text"
                placeholder="2FA code (optional)"
                value={twoFactor}
                onChange={(e) => setTwoFactor(e.target.value)}
                className={inputClass}
              />
              <button
                type="submit"
                className="text-xs px-3 py-1.5 rounded bg-[var(--ct-accent)] text-[var(--ct-bg)] hover:opacity-90"
              >
                Sign in
              </button>
            </form>
          ) : (
            <form onSubmit={submitApiKeyLogin} className="space-y-2">
              <div className="text-[10px] text-[var(--ct-muted-fg)]">
                Create a personal API key at vault.bitwarden.com → Security → Keys.
              </div>
              <input
                type="text"
                placeholder="client_id"
                value={apiClientId}
                onChange={(e) => setApiClientId(e.target.value)}
                className={inputClass}
                required
              />
              <input
                type="password"
                placeholder="client_secret"
                value={apiClientSecret}
                onChange={(e) => setApiClientSecret(e.target.value)}
                className={inputClass}
                required
              />
              <input
                type="password"
                placeholder="master password (still required to unlock)"
                autoComplete="current-password"
                value={masterPassword}
                onChange={(e) => setMasterPassword(e.target.value)}
                className={inputClass}
                required
              />
              <button
                type="submit"
                className="text-xs px-3 py-1.5 rounded bg-[var(--ct-accent)] text-[var(--ct-bg)] hover:opacity-90"
              >
                Sign in with API key
              </button>
            </form>
          )}
        </div>
      )}

      {/* Locked → just need password. */}
      {status?.status?.status === "locked" && backend === "bitwarden" && (
        <div className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4 space-y-3">
          <div className="text-sm font-medium text-[var(--ct-fg)]">Unlock vault</div>
          <form onSubmit={submitPasswordLogin} className="space-y-2">
            <input
              type="password"
              placeholder="master password"
              autoComplete="current-password"
              value={masterPassword}
              onChange={(e) => setMasterPassword(e.target.value)}
              className={inputClass}
              required
            />
            <button
              type="submit"
              className="text-xs px-3 py-1.5 rounded bg-[var(--ct-accent)] text-[var(--ct-bg)] hover:opacity-90"
            >
              Unlock
            </button>
          </form>
        </div>
      )}

      {/* Mock backend has no real auth — just an "unlock" button to flip the flag. */}
      {backend === "mock" && status?.status?.status !== "unlocked" && (
        <div className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4 space-y-2">
          <div className="text-sm font-medium text-[var(--ct-fg)]">Mock backend (testing only)</div>
          <div className="text-xs text-[var(--ct-muted-fg)]">
            In-memory store, no auth. Anything you "save" lives until the daemon restarts. Use{" "}
            <strong>Local secret storage</strong> for real secrets.
          </div>
          <button
            onClick={() => {
              setMasterPassword("dev-only");
              submitPasswordLogin(new Event("submit") as unknown as React.FormEvent);
            }}
            className="text-xs px-3 py-1.5 rounded bg-[var(--ct-accent)] text-[var(--ct-bg)] hover:opacity-90"
          >
            Activate mock store
          </button>
        </div>
      )}

      {/* Default scope + recent items, only shown when unlocked. */}
      {status?.status?.status === "unlocked" && (
        <>
          <div className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4 space-y-3">
            <div className="flex items-center justify-between">
              <div className="text-sm font-medium text-[var(--ct-fg)]">
                {backend === "bitwarden" ? "Default scope" : "Connected"}
              </div>
              {status.status.endpoint && (
                <span className="text-[10px] font-mono text-[var(--ct-muted-fg)]">
                  {status.status.endpoint}
                </span>
              )}
            </div>
            {backend === "bitwarden" && (
              <>
                <div className="text-xs text-[var(--ct-muted-fg)]">
                  Where new secrets land when no <code className="font-mono">--org</code> /{" "}
                  <code className="font-mono">--collection</code> is given.
                </div>
                <div className="space-y-1">
                  <div className="text-[10px] uppercase tracking-wider text-[var(--ct-muted)]">Organization</div>
                  <select
                    value={cfg.default_organization}
                    onChange={(e) => setDefaultOrg(e.target.value)}
                    className={inputClass}
                  >
                    <option value="">Personal vault</option>
                    {orgs.map((o) => (
                      <option key={o.id} value={o.id}>
                        {o.name}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="space-y-1">
                  <div className="text-[10px] uppercase tracking-wider text-[var(--ct-muted)]">Collection</div>
                  <select
                    value={cfg.default_collection}
                    onChange={(e) => setDefaultColl(e.target.value)}
                    className={inputClass}
                    disabled={!colls.length}
                  >
                    <option value="">(none)</option>
                    {colls.map((c) => (
                      <option key={c.id} value={c.id}>
                        {c.name}
                      </option>
                    ))}
                  </select>
                </div>
              </>
            )}
            <div className="flex gap-2 pt-1">
              <button
                onClick={lock}
                className="text-xs px-3 py-1.5 rounded border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]"
              >
                Lock
              </button>
              <button
                onClick={logout}
                className="text-xs px-3 py-1.5 rounded border border-[var(--ct-red)]/40 text-[var(--ct-red)] hover:bg-[var(--ct-red)]/10"
              >
                Sign out
              </button>
            </div>
          </div>

          <div className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4 space-y-2">
            <div className="flex items-center justify-between">
              <div className="text-sm font-medium text-[var(--ct-fg)]">Recently saved</div>
              <button
                onClick={refreshItems}
                className="text-[10px] px-2 py-0.5 rounded border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]"
              >
                refresh
              </button>
            </div>
            {items.length === 0 ? (
              <div className="text-[11px] text-[var(--ct-muted-fg)]">
                No secrets saved yet. Use{" "}
                <code className="font-mono text-[var(--ct-fg)]">codeterm mem secret save --name X</code>{" "}
                from any pane.
              </div>
            ) : (
              <div className="space-y-1">
                {items.map((it) => (
                  <div key={it.item_id} className="flex items-center justify-between text-[11px] font-mono">
                    <span className="text-[var(--ct-fg)] truncate">{it.name}</span>
                    <span className="text-[var(--ct-muted)] truncate ml-3">
                      {it.folder ? `folder:${it.folder.slice(0, 8)} ` : ""}
                      {it.organization ? `org:${it.organization.slice(0, 8)}` : ""}
                    </span>
                  </div>
                ))}
              </div>
            )}
          </div>
        </>
      )}
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

interface ShellDiagnostics {
  platform: string;
  pwsh_installed: boolean;
  pwsh_path?: string | null;
  powershell_path?: string | null;
  current_shell: string;
}

function ShellDiagnosticsBanner({ onDismissSettings }: { onDismissSettings: () => void }) {
  const [diag, setDiag] = useState<ShellDiagnostics | null>(null);

  useEffect(() => {
    invoke<ShellDiagnostics>("get_shell_diagnostics").then(setDiag).catch(() => {});
  }, []);

  if (!diag || diag.platform !== "windows" || diag.pwsh_installed) return null;

  const handleInstall = async () => {
    try {
      await invoke("suggest_install_pwsh");
      onDismissSettings();
    } catch (e) {
      console.warn("suggest_install_pwsh failed:", e);
    }
  };

  const handleRestart = async () => {
    try {
      await invoke("restart_app");
    } catch (e) {
      console.warn("restart_app failed:", e);
    }
  };

  return (
    <div
      className="rounded-lg border p-3 flex items-start gap-3"
      style={{
        background: "color-mix(in srgb, var(--ct-yellow) 8%, var(--ct-bg))",
        borderColor: "color-mix(in srgb, var(--ct-yellow) 35%, transparent)",
      }}
    >
      <div className="w-8 h-8 rounded-md flex items-center justify-center shrink-0"
        style={{ background: "color-mix(in srgb, var(--ct-yellow) 20%, transparent)", color: "var(--ct-yellow)" }}>
        <Sliders size={16} />
      </div>
      <div className="flex-1 min-w-0">
        <div className="text-[13px] font-medium text-[var(--ct-fg)]">Faster shell available</div>
        <div className="text-[11px] text-[var(--ct-muted-fg)] mt-0.5">
          You're on Windows PowerShell 5.1 which cold-starts slowly and stalls on Defender AMSI.
          PowerShell 7 (pwsh) starts 5–10× faster. CodeTerm will auto-detect it after install.
        </div>
        <div className="flex gap-1.5 mt-2">
          <button
            onClick={handleInstall}
            className="text-[11px] px-2.5 py-1 rounded border bg-[var(--ct-accent)] text-[var(--ct-bg)] border-[var(--ct-accent)] hover:opacity-90"
          >
            Install via winget
          </button>
          <button
            onClick={handleRestart}
            className="text-[11px] px-2.5 py-1 rounded border bg-transparent text-[var(--ct-muted-fg)] border-[var(--ct-border)] hover:text-[var(--ct-fg)]"
          >
            Restart CodeTerm
          </button>
        </div>
      </div>
    </div>
  );
}
