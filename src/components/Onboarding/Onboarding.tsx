import { useState, useEffect, useCallback, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { AppConfig, ThemeConfig } from "../../lib/types";

const isMacPlatform = navigator.platform.toUpperCase().includes("MAC");

interface OnboardingProps {
  config: AppConfig;
  theme: ThemeConfig;
  onComplete: (config: AppConfig) => void;
}

const TOTAL_STEPS = 5;

export function Onboarding({ config, theme, onComplete }: OnboardingProps) {
  const [step, setStep] = useState(0);
  const [entered, setEntered] = useState(false);
  const [leaving, setLeaving] = useState(false);
  const [themes, setThemes] = useState<string[]>([]);
  const [selectedTheme, setSelectedTheme] = useState(theme.name);
  const [skillsInstalled, setSkillsInstalled] = useState(false);
  const [hooksInstalled, setHooksInstalled] = useState(false);
  const [rulesInstalled, setRulesInstalled] = useState(false);
  const [gaEnabled, setGaEnabled] = useState(config.general_agent?.enabled ?? true);
  const [gaAutostart, setGaAutostart] = useState(false);
  const [apiToken, setApiToken] = useState<string | null>(config.api?.token ?? null);
  const [networkInfo, setNetworkInfo] = useState<{ api_url: string; tailscale_url: string | null } | null>(null);
  const [orchestratorPrompt, setOrchestratorPrompt] = useState("");

  useEffect(() => {
    requestAnimationFrame(() => requestAnimationFrame(() => setEntered(true)));
    invoke<string[]>("list_themes").then(setThemes).catch(() => {});
    // Check what's already installed
    invoke<Array<{ id: string; installed: boolean }>>("list_skills")
      .then((s) => { if (s.some((sk) => sk.id === "codeterm" && sk.installed)) setSkillsInstalled(true); })
      .catch(() => {});
    invoke<{ installed: boolean }>("check_rule_installed")
      .then((r) => { if (r.installed) setRulesInstalled(true); })
      .catch(() => {});
    invoke<{ installed: boolean }>("check_claude_hooks")
      .then((r) => { if (r.installed) setHooksInstalled(true); })
      .catch(() => {});
  }, []);

  const finish = useCallback((cfg: AppConfig) => {
    setLeaving(true);
    const finalCfg = { ...cfg, general_agent: { ...cfg.general_agent, enabled: gaEnabled }, onboarding_completed: true };
    if (gaAutostart) invoke("set_autostart_enabled", { enabled: true }).catch(() => {});
    setTimeout(() => onComplete(finalCfg), 400);
  }, [onComplete, gaEnabled, gaAutostart]);

  const next = useCallback(() => {
    if (step < TOTAL_STEPS - 1) setStep((s) => s + 1);
    else finish(config);
  }, [step, config, finish]);

  const skip = useCallback(() => finish(config), [config, finish]);

  const applyTheme = useCallback(async (name: string) => {
    setSelectedTheme(name);
    try {
      await invoke("update_config", {
        config: { ...config, theme: { ...config.theme, name } },
      });
    } catch {}
  }, [config]);

  const installSkill = useCallback(async () => {
    try { await invoke("install_skill", { skillId: "codeterm" }); setSkillsInstalled(true); } catch {}
  }, []);
  const installHooks = useCallback(async () => {
    try { await invoke("install_claude_hooks", { apiPort: null }); setHooksInstalled(true); } catch {}
  }, []);
  const installRules = useCallback(async () => {
    try { await invoke("install_project_rules"); setRulesInstalled(true); } catch {}
  }, []);

  // Auto-initialize network info when reaching the network step
  useEffect(() => {
    if (step !== 4) return;
    (async () => {
      try {
        // Generate token if we don't have one yet
        let token = apiToken;
        if (!token) {
          token = await invoke<string>("generate_api_token");
          setApiToken(token);
          await invoke("update_config", {
            config: { ...config, api: { ...config.api, enabled: true, token } },
          });
        }
        // Fetch network info
        const net = await invoke<{ api_url: string; tailscale_url: string | null }>("get_network_info");
        setNetworkInfo(net);
        // Build orchestrator prompt
        const base = await invoke<string>("get_skills_prompt");
        const url = net.tailscale_url || net.api_url;
        setOrchestratorPrompt(`CodeTerm API: ${url}\nAuthorization: Bearer ${token}\n\n${base}`);
      } catch {}
    })();
  }, [step]); // eslint-disable-line react-hooks/exhaustive-deps

  const disableNetwork = useCallback(async () => {
    try {
      await invoke("update_config", {
        config: { ...config, api: { ...config.api, enabled: false } },
      });
      setApiToken(null);
      setNetworkInfo(null);
      setOrchestratorPrompt("");
    } catch {}
  }, [config]);

  // Derived
  const progress = ((step + 1) / TOTAL_STEPS) * 100;
  const opacity = entered && !leaving ? 1 : 0;

  // Theme-derived colors
  const c = useMemo(() => ({
    bg: theme.background,
    fg: theme.foreground,
    muted: theme.foreground + "90",
    dimmed: theme.foreground + "60",
    accent: theme.blue,
    green: theme.green,
    surface: adjustBrightness(theme.background, 12),
    border: adjustBrightness(theme.background, 24),
    hover: adjustBrightness(theme.background, 18),
    glow: theme.blue + "08",
  }), [theme]);

  return (
    <div
      className="fixed inset-0 z-[200] flex items-center justify-center transition-all duration-500 ease-out"
      style={{
        opacity,
        backgroundColor: c.bg,
      }}
    >
      {/* Subtle grid pattern */}
      <div
        className="absolute inset-0 pointer-events-none"
        style={{
          backgroundImage: `radial-gradient(${c.border} 1px, transparent 1px)`,
          backgroundSize: "24px 24px",
          opacity: 0.4,
        }}
      />

      {/* Accent glow at top */}
      <div
        className="absolute top-0 left-1/2 -translate-x-1/2 w-[600px] h-[300px] pointer-events-none"
        style={{
          background: `radial-gradient(ellipse at center, ${c.accent}12 0%, transparent 70%)`,
        }}
      />

      {/* Content */}
      <div className="relative z-10 w-full h-full flex flex-col">
        {/* Titlebar: drag region + progress + window controls */}
        <div
          className="flex items-center h-9 shrink-0 select-none"
          data-tauri-drag-region
          style={{ backgroundColor: "transparent" }}
        >
          {/* macOS: traffic lights on the left */}
          {isMacPlatform && <OnboardingMacLights c={c} />}

          {/* Left: progress */}
          <div className="flex items-center gap-3 pl-4" data-tauri-drag-region>
            <span
              className="text-[11px] font-mono tracking-wider uppercase"
              style={{ color: c.dimmed }}
              data-tauri-drag-region
            >
              Setup {step + 1}/{TOTAL_STEPS}
            </span>
            <div className="w-32 h-[3px] rounded-full overflow-hidden" style={{ backgroundColor: c.border }}>
              <div
                className="h-full rounded-full transition-all duration-500 ease-out"
                style={{ width: `${progress}%`, backgroundColor: c.accent }}
              />
            </div>
          </div>

          {/* Center spacer — drag region */}
          <div className="flex-1" data-tauri-drag-region />

          {/* Windows/Linux: controls on the right */}
          {!isMacPlatform && <WindowControls c={c} />}
        </div>

        {/* Main content — centered, full height */}
        <div className="flex-1 flex items-center justify-center px-6">
          <div
            className="w-full max-w-xl transition-all duration-300 ease-out"
            key={step}
            style={{ animation: "fadeSlideIn 0.35s ease-out" }}
          >
            {step === 0 && <StepWelcome c={c} />}
            {step === 1 && (
              <StepTheme c={c} themes={themes} selected={selectedTheme} onSelect={applyTheme} />
            )}
            {step === 2 && (
              <StepSetup c={c}
                skillsInstalled={skillsInstalled} hooksInstalled={hooksInstalled} rulesInstalled={rulesInstalled}
                onInstallSkill={installSkill} onInstallHooks={installHooks} onInstallRules={installRules}
              />
            )}
            {step === 3 && (
              <StepAgents c={c}
                gaEnabled={gaEnabled} onGaEnabled={setGaEnabled}
                autostart={gaAutostart} onAutostart={setGaAutostart}
              />
            )}
            {step === 4 && (
              <StepNetwork c={c}
                token={apiToken} networkInfo={networkInfo}
                orchestratorPrompt={orchestratorPrompt}
                onDisable={disableNetwork}
              />
            )}
          </div>
        </div>

        {/* Bottom navigation */}
        <div className="flex flex-col items-center gap-2.5 px-6 pb-6 pt-2">
          <div className="relative flex items-center justify-center">
            {step > 0 && (
              <button
                onClick={() => setStep((s) => s - 1)}
                className="absolute right-full mr-3 px-2 py-2 rounded-md transition-colors flex items-center"
                style={{ color: c.fg + "b0", backgroundColor: c.surface }}
                onMouseEnter={(e) => { e.currentTarget.style.color = c.fg; e.currentTarget.style.backgroundColor = c.hover; }}
                onMouseLeave={(e) => { e.currentTarget.style.color = c.fg + "b0"; e.currentTarget.style.backgroundColor = c.surface; }}
              >
                <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M10 3L5 8l5 5" />
                </svg>
              </button>
            )}
            <button
              onClick={next}
              className="text-xs px-6 py-2 rounded-md font-medium transition-all duration-200"
              style={{ backgroundColor: c.accent, color: "#fff" }}
              onMouseEnter={(e) => (e.currentTarget.style.filter = "brightness(1.15)")}
              onMouseLeave={(e) => (e.currentTarget.style.filter = "none")}
            >
              {step === TOTAL_STEPS - 1 ? "Get Started" : "Continue"}
            </button>
          </div>
          <button
            onClick={skip}
            className="text-[11px] transition-colors underline underline-offset-2 decoration-dotted"
            style={{ color: c.dimmed }}
            onMouseEnter={(e) => (e.currentTarget.style.color = c.fg)}
            onMouseLeave={(e) => (e.currentTarget.style.color = c.dimmed)}
          >
            Skip setup
          </button>
        </div>
      </div>

      <style>{`
        @keyframes fadeSlideIn {
          from { opacity: 0; transform: translateY(12px); }
          to { opacity: 1; transform: translateY(0); }
        }
      `}</style>
    </div>
  );
}

// ── Color helper ────────────────────────────────────────────────────

type C = { bg: string; fg: string; muted: string; dimmed: string; accent: string; green: string; surface: string; border: string; hover: string; glow: string };

function adjustBrightness(hex: string, amount: number): string {
  const clean = hex.replace("#", "");
  const r = parseInt(clean.substring(0, 2), 16);
  const g = parseInt(clean.substring(2, 4), 16);
  const b = parseInt(clean.substring(4, 6), 16);
  const dir = (r + g + b) / 3 > 128 ? -1 : 1;
  const nr = Math.min(255, Math.max(0, r + amount * dir));
  const ng = Math.min(255, Math.max(0, g + amount * dir));
  const nb = Math.min(255, Math.max(0, b + amount * dir));
  return `#${nr.toString(16).padStart(2, "0")}${ng.toString(16).padStart(2, "0")}${nb.toString(16).padStart(2, "0")}`;
}

// ── Step 1: Welcome ─────────────────────────────────────────────────

function StepWelcome({ c }: { c: C }) {
  return (
    <div className="text-center">
      {/* Logo */}
      <div
        className="inline-flex items-center justify-center w-16 h-16 rounded-2xl mb-6"
        style={{ backgroundColor: c.accent + "15" }}
      >
        <span className="text-2xl font-mono font-bold">
          <span style={{ color: c.accent }}>&lt;</span>
          <span style={{ color: "#d4a574" }}>/</span>
          <span style={{ color: c.accent }}>&gt;</span>
        </span>
      </div>

      <h1 className="text-2xl font-semibold mb-2" style={{ color: c.fg }}>
        Welcome to CodeTerm
      </h1>
      <p className="text-sm mb-10" style={{ color: c.muted }}>
        AI-first terminal multiplexer for developers
      </p>

      <div className="grid grid-cols-2 gap-3 text-left max-w-md mx-auto">
        <FeatureCard c={c} icon={"\u2726"} title="General Agent" desc="Always-on AI assistant — starts with CodeTerm, ready for anything" />
        <FeatureCard c={c} icon="+" title="Agent orchestration" desc="Spawn teams of AI agents that work in parallel and report back" />
        <FeatureCard c={c} icon="@" title="Cron jobs" desc="Schedule recurring AI tasks — agents run on a cron and report results" />
        <FeatureCard c={c} icon=">" title="Canvas panels" desc="Visualizations and localhost apps inline with terminals" />
        <FeatureCard c={c} icon="!" title="Messaging" desc="Telegram, Discord — talk to your agent from your phone" />
        <FeatureCard c={c} icon="*" title="Model-agnostic" desc="Claude, Codex, Gemini, local models — any agent that runs in a terminal" />
      </div>

      <p className="text-[11px] mt-8" style={{ color: c.dimmed }}>
        Just 4 quick steps. Everything is also in Settings (Ctrl+,).
      </p>
    </div>
  );
}

function FeatureCard({ c, icon, title, desc, iconColor }: { c: C; icon: string; title: string; desc: string; iconColor?: string }) {
  return (
    <div
      className="px-4 py-3 rounded-lg"
      style={{ backgroundColor: c.surface + "30" }}
    >
      <div className="flex items-center gap-2 mb-1">
        <span className="text-xs font-mono" style={{ color: iconColor || c.accent }}>{icon}</span>
        <span className="text-xs font-medium" style={{ color: c.fg }}>{title}</span>
      </div>
      <p className="text-[11px] leading-relaxed" style={{ color: c.muted }}>{desc}</p>
    </div>
  );
}

// ── Step 2: Theme ───────────────────────────────────────────────────

function StepTheme({
  c, themes, selected, onSelect,
}: { c: C; themes: string[]; selected: string; onSelect: (n: string) => void }) {
  return (
    <div className="text-center">
      <h2 className="text-xl font-semibold mb-1" style={{ color: c.fg }}>
        Choose your look
      </h2>
      <p className="text-sm mb-6" style={{ color: c.muted }}>
        Pick a theme. Your agent can also switch it or create new any time.
      </p>

      <div className="grid grid-cols-4 gap-2 max-w-lg mx-auto">
        {themes.map((t) => {
          const active = selected === t;
          return (
            <button
              key={t}
              onClick={() => onSelect(t)}
              className="relative text-left px-3 py-2.5 rounded-lg border text-xs transition-all duration-200"
              style={{
                borderColor: active ? c.accent : c.border,
                backgroundColor: active ? c.accent + "10" : "transparent",
                color: c.fg,
              }}
            >
              <span className="font-medium">{formatName(t)}</span>
              {active && (
                <span
                  className="absolute top-1.5 right-2 text-[10px]"
                  style={{ color: c.accent }}
                >
                  &#10003;
                </span>
              )}
            </button>
          );
        })}
      </div>

      {/* Live terminal preview */}
      <TerminalPreview c={c} />
    </div>
  );
}

function TerminalPreview({ c }: { c: C }) {
  return (
    <div
      className="mt-6 mx-auto max-w-lg rounded-lg overflow-hidden border text-left font-mono text-[11px] leading-relaxed"
      style={{ borderColor: c.border, backgroundColor: c.bg }}
    >
      {/* Fake titlebar */}
      <div
        className="flex items-center gap-1.5 px-3 py-1.5"
        style={{ backgroundColor: c.surface }}
      >
        <div className="w-2 h-2 rounded-full" style={{ backgroundColor: c.muted + "60" }} />
        <div className="w-2 h-2 rounded-full" style={{ backgroundColor: c.muted + "60" }} />
        <div className="w-2 h-2 rounded-full" style={{ backgroundColor: c.muted + "60" }} />
        <span className="ml-2 text-[10px]" style={{ color: c.dimmed }}>Terminal</span>
      </div>
      {/* Fake terminal output */}
      <div className="px-3 py-2.5 space-y-0.5">
        <div>
          <span style={{ color: c.green }}>$</span>{" "}
          <span style={{ color: c.fg }}>claude &quot;fix the auth bug&quot;</span>
        </div>
        <div style={{ color: c.muted }}>
          Analyzing codebase...
        </div>
        <div>
          <span style={{ color: c.accent }}>{">"}</span>{" "}
          <span style={{ color: c.fg }}>Found issue in</span>{" "}
          <span style={{ color: c.accent }}>src/auth/middleware.ts</span>
        </div>
        <div>
          <span style={{ color: c.accent }}>{">"}</span>{" "}
          <span style={{ color: c.green }}>Fixed</span>{" "}
          <span style={{ color: c.fg }}>— token validation now checks expiry</span>
        </div>
        <div>
          <span style={{ color: c.green }}>$</span>{" "}
          <span style={{ color: c.muted }}>&#x2588;</span>
        </div>
      </div>
    </div>
  );
}

// ── Step 3: Setup ───────────────────────────────────────────────────

function StepSetup({
  c, skillsInstalled, hooksInstalled, rulesInstalled,
  onInstallSkill, onInstallHooks, onInstallRules,
}: {
  c: C;
  skillsInstalled: boolean; hooksInstalled: boolean; rulesInstalled: boolean;
  onInstallSkill: () => void; onInstallHooks: () => void; onInstallRules: () => void;
}) {
  const allDone = skillsInstalled && hooksInstalled && rulesInstalled;

  return (
    <div className="text-center">
      <h2 className="text-xl font-semibold mb-1" style={{ color: c.fg }}>
        AI integration
      </h2>
      <p className="text-sm mb-8" style={{ color: c.muted }}>
        Connect Claude Code with CodeTerm for the full experience.
      </p>

      <div className="space-y-2 max-w-md mx-auto text-left">
        <SetupRow c={c}
          title="CodeTerm skill"
          desc="Split panes, create workspaces, open canvas panels, spawn agents — all from Claude"
          installed={skillsInstalled} onInstall={onInstallSkill}
        />
        <SetupRow c={c}
          title="Notification hooks"
          desc="Get desktop alerts when Claude finishes a task or needs your permission to proceed"
          installed={hooksInstalled} onInstall={onInstallHooks}
        />
        <SetupRow c={c}
          title="Project rules"
          desc="Claude auto-renames tabs, uses workspaces instead of mkdir, opens browser panels inline"
          installed={rulesInstalled} onInstall={onInstallRules}
        />
      </div>

      {allDone && (
        <p className="text-xs mt-6" style={{ color: c.green }}>
          &#10003; All set up
        </p>
      )}
    </div>
  );
}

function SetupRow({
  c, title, desc, installed, onInstall,
}: { c: C; title: string; desc: string; installed: boolean; onInstall: () => void }) {
  return (
    <div
      className="flex items-center justify-between px-4 py-3 rounded-lg border transition-colors"
      style={{
        borderColor: installed ? c.green + "40" : c.border,
        backgroundColor: installed ? c.green + "08" : c.surface + "60",
      }}
    >
      <div className="min-w-0 mr-4">
        <div className="text-xs font-medium" style={{ color: c.fg }}>{title}</div>
        <div className="text-[11px]" style={{ color: c.muted }}>{desc}</div>
      </div>
      {installed ? (
        <span className="text-xs shrink-0 font-medium" style={{ color: c.green }}>
          &#10003;
        </span>
      ) : (
        <button
          onClick={onInstall}
          className="shrink-0 text-[11px] px-3 py-1.5 rounded-md font-medium transition-all"
          style={{ backgroundColor: c.accent, color: "#fff" }}
          onMouseEnter={(e) => (e.currentTarget.style.filter = "brightness(1.15)")}
          onMouseLeave={(e) => (e.currentTarget.style.filter = "none")}
        >
          Install
        </button>
      )}
    </div>
  );
}

// ── Step 3: General Agent ──────────────────────────────────────────

function OnbSwitch({ checked, onChange, c }: { checked: boolean; onChange: (v: boolean) => void; c: C }) {
  return (
    <button type="button" role="switch" aria-checked={checked} onClick={() => onChange(!checked)}
      className="relative inline-flex h-5 w-9 items-center rounded-full transition-colors shrink-0"
      style={{ backgroundColor: checked ? c.green : c.border }}>
      <span className="inline-block h-3.5 w-3.5 rounded-full bg-white transition-transform"
        style={{ transform: checked ? "translateX(16px)" : "translateX(2px)" }} />
    </button>
  );
}

function StepAgents({ c, gaEnabled, onGaEnabled, autostart, onAutostart }: {
  c: C; gaEnabled: boolean; onGaEnabled: (v: boolean) => void; autostart: boolean; onAutostart: (v: boolean) => void;
}) {
  return (
    <div className="text-center">
      <div className="inline-flex items-center justify-center w-14 h-14 rounded-full mb-6"
        style={{ backgroundColor: c.accent + "15" }}>
        <span className="text-2xl" style={{ color: c.accent }}>&#x2726;</span>
      </div>
      <h2 className="text-xl font-semibold mb-1" style={{ color: c.fg }}>General Agent</h2>
      <p className="text-sm mb-8" style={{ color: c.muted }}>
        An always-on AI assistant that lives in a protected workspace.
        It starts with CodeTerm and is always ready for your tasks.
      </p>
      <div className="max-w-md mx-auto space-y-2 text-left">
        <div className="flex items-center justify-between px-4 py-3 rounded-lg border" style={{
          borderColor: gaEnabled ? c.green + "40" : c.border,
          backgroundColor: gaEnabled ? c.green + "08" : c.surface + "60",
        }}>
          <div className="min-w-0 mr-4">
            <div className="text-xs font-medium" style={{ color: c.fg }}>Enable General Agent</div>
            <div className="text-[11px]" style={{ color: c.muted }}>Creates a "General" workspace with your main AI assistant</div>
          </div>
          <OnbSwitch checked={gaEnabled} onChange={onGaEnabled} c={c} />
        </div>
        {gaEnabled && (
          <>
            <div className="flex items-center justify-between px-4 py-3 rounded-lg border" style={{
              borderColor: c.accent + "30", backgroundColor: c.surface + "60",
            }}>
              <div className="min-w-0 mr-4">
                <div className="text-xs font-medium" style={{ color: c.fg }}>Auto-approve tool calls</div>
                <div className="text-[11px]" style={{ color: c.muted }}>Agent runs with --dangerously-skip-permissions (enabled by default)</div>
              </div>
              <span className="text-[10px] shrink-0" style={{ color: c.green }}>&#10003; On</span>
            </div>
            <div className="flex items-center justify-between px-4 py-3 rounded-lg border" style={{
              borderColor: c.border, backgroundColor: c.surface + "60",
            }}>
              <div className="min-w-0 mr-4">
                <div className="text-xs font-medium" style={{ color: c.fg }}>Launch at login</div>
                <div className="text-[11px]" style={{ color: c.muted }}>Start CodeTerm automatically when you log in</div>
              </div>
              <OnbSwitch checked={autostart} onChange={onAutostart} c={c} />
            </div>
          </>
        )}
      </div>
      {gaEnabled && (
        <p className="text-[11px] mt-6 max-w-sm mx-auto" style={{ color: c.dimmed }}>
          Your General Agent can set up Telegram and other integrations for you. Just ask it after onboarding.
        </p>
      )}
    </div>
  );
}

// ── Step 4: Network & Orchestrator ───────────────────────────────────

function StepNetwork({
  c, token, networkInfo, orchestratorPrompt, onDisable,
}: {
  c: C;
  token: string | null;
  networkInfo: { api_url: string; tailscale_url: string | null } | null;
  orchestratorPrompt: string;
  onDisable: () => void;
}) {
  const [copied, setCopied] = useState(false);
  const [tokenCopied, setTokenCopied] = useState(false);

  return (
    <div className="text-center">
      <div
        className="inline-flex items-center justify-center w-14 h-14 rounded-full mb-6"
        style={{ backgroundColor: c.accent + "15" }}
      >
        <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke={c.accent} strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
          <circle cx="12" cy="12" r="10" />
          <path d="M2 12h20" />
          <path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z" />
        </svg>
      </div>

      <h2 className="text-xl font-semibold mb-1" style={{ color: c.fg }}>
        Network API
      </h2>
      <p className="text-sm mb-6" style={{ color: c.muted }}>
        CodeTerm's REST API is ready. External tools and orchestrators can control your terminal remotely.
      </p>

      <div className="max-w-md mx-auto space-y-3 text-left">
        {/* Status + address */}
        <div
          className="flex items-center justify-between px-4 py-3 rounded-lg border"
          style={{ borderColor: c.green + "40", backgroundColor: c.green + "08" }}
        >
          <div>
            <div className="flex items-center gap-2">
              <div className="w-2 h-2 rounded-full" style={{ backgroundColor: c.green }} />
              <span className="text-xs font-medium" style={{ color: c.fg }}>API Active</span>
            </div>
            <div className="text-[11px] mt-0.5" style={{ color: c.muted }}>
              {networkInfo?.tailscale_url
                ? `${networkInfo.api_url} (local) · ${networkInfo.tailscale_url} (Tailscale)`
                : networkInfo?.api_url
                  ? networkInfo.api_url
                  : "Initializing..."}
            </div>
          </div>
          <button
            onClick={onDisable}
            className="shrink-0 text-[11px] px-3 py-1.5 rounded-md font-medium transition-all"
            style={{ backgroundColor: c.surface, color: c.muted, borderWidth: 1, borderColor: c.border }}
            onMouseEnter={(e) => (e.currentTarget.style.filter = "brightness(1.15)")}
            onMouseLeave={(e) => (e.currentTarget.style.filter = "none")}
          >
            Disable
          </button>
        </div>

        {/* Token */}
        {token && (
          <div
            className="px-4 py-3 rounded-lg border"
            style={{ borderColor: c.border, backgroundColor: c.surface + "60" }}
          >
            <div className="text-[11px] font-medium mb-1.5" style={{ color: c.muted }}>API Token</div>
            <div className="flex items-center gap-2">
              <code
                className="flex-1 text-[10px] font-mono px-2 py-1 rounded truncate"
                style={{ backgroundColor: c.bg, color: c.fg }}
              >
                {token}
              </code>
              <button
                onClick={() => {
                  navigator.clipboard.writeText(token);
                  setTokenCopied(true);
                  setTimeout(() => setTokenCopied(false), 2000);
                }}
                className="shrink-0 text-[10px] px-2 py-1 rounded transition-colors"
                style={{ backgroundColor: c.border, color: c.fg }}
                onMouseEnter={(e) => (e.currentTarget.style.backgroundColor = c.hover)}
                onMouseLeave={(e) => (e.currentTarget.style.backgroundColor = c.border)}
              >
                {tokenCopied ? "Copied!" : "Copy"}
              </button>
            </div>
          </div>
        )}

        {/* Orchestrator prompt */}
        <div
          className="px-4 py-3 rounded-lg border"
          style={{ borderColor: c.border, backgroundColor: c.surface + "60" }}
        >
          <div className="text-[11px] font-medium mb-1" style={{ color: c.fg }}>External Orchestrator Prompt</div>
          <div className="text-[10px] mb-2" style={{ color: c.muted }}>
            Give this to a gateway or any AI orchestrator — it includes the API URL, auth token, and all available commands so it can control CodeTerm.
          </div>
          <div className="relative">
            <pre
              className="rounded p-2 text-[9px] font-mono whitespace-pre-wrap max-h-[100px] overflow-y-auto leading-relaxed"
              style={{ backgroundColor: c.bg, color: c.muted }}
            >
              {orchestratorPrompt || "Loading..."}
            </pre>
            <button
              onClick={() => {
                navigator.clipboard.writeText(orchestratorPrompt).then(() => {
                  setCopied(true);
                  setTimeout(() => setCopied(false), 2000);
                });
              }}
              className="absolute top-1.5 right-1.5 px-2 py-0.5 text-[9px] rounded transition-colors"
              style={{ backgroundColor: c.border, color: c.fg }}
              onMouseEnter={(e) => (e.currentTarget.style.backgroundColor = c.hover)}
              onMouseLeave={(e) => (e.currentTarget.style.backgroundColor = c.border)}
            >
              {copied ? "Copied!" : "Copy"}
            </button>
          </div>
        </div>

        <div className="text-[10px] text-center" style={{ color: c.dimmed }}>
          You can regenerate the token anytime in Settings &rarr; Network & API.
        </div>
      </div>
    </div>
  );
}

// ── macOS traffic lights (onboarding) ───────────────────────────────

function OnboardingMacLights({ c }: { c: C }) {
  const appWindow = getCurrentWindow();
  const close = useCallback(() => appWindow.close(), [appWindow]);
  const minimize = useCallback(() => appWindow.minimize(), [appWindow]);
  const maximize = useCallback(() => appWindow.toggleMaximize(), [appWindow]);
  const [hover, setHover] = useState(false);

  void c;
  return (
    <div
      className="flex items-center gap-2 pl-3.5 pr-1 h-full"
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
    >
      <button onClick={close} className="w-3 h-3 rounded-full flex items-center justify-center" style={{ backgroundColor: "#ff5f57" }} title="Close">
        {hover && <svg width="6" height="6" viewBox="0 0 6 6" fill="none" stroke="#4d0000" strokeWidth="1.2" strokeLinecap="round"><line x1="1" y1="1" x2="5" y2="5" /><line x1="5" y1="1" x2="1" y2="5" /></svg>}
      </button>
      <button onClick={minimize} className="w-3 h-3 rounded-full flex items-center justify-center" style={{ backgroundColor: "#febc2e" }} title="Minimize">
        {hover && <svg width="6" height="2" viewBox="0 0 6 2" fill="none" stroke="#995700" strokeWidth="1.2" strokeLinecap="round"><line x1="1" y1="1" x2="5" y2="1" /></svg>}
      </button>
      <button onClick={maximize} className="w-3 h-3 rounded-full flex items-center justify-center" style={{ backgroundColor: "#28c840" }} title="Maximize">
        {hover && <svg width="6" height="6" viewBox="0 0 6 6" fill="none" stroke="#006500" strokeWidth="1.1" strokeLinecap="round"><polyline points="1,3.5 1,1 3.5,1" /><polyline points="5,2.5 5,5 2.5,5" /></svg>}
      </button>
    </div>
  );
}

// ── Window controls (Windows/Linux) ─────────────────────────────────

function WindowControls({ c }: { c: C }) {
  const appWindow = getCurrentWindow();
  const minimize = useCallback(() => appWindow.minimize(), [appWindow]);
  const maximize = useCallback(() => appWindow.toggleMaximize(), [appWindow]);
  const close = useCallback(() => appWindow.close(), [appWindow]);

  return (
    <div className="flex items-center h-full">
      <button
        onClick={minimize}
        className="inline-flex items-center justify-center w-12 h-full transition-colors"
        style={{ color: c.muted }}
        onMouseEnter={(e) => { e.currentTarget.style.backgroundColor = c.border; e.currentTarget.style.color = c.fg; }}
        onMouseLeave={(e) => { e.currentTarget.style.backgroundColor = "transparent"; e.currentTarget.style.color = c.muted; }}
        title="Minimize"
      >
        <svg width="10" height="1" viewBox="0 0 10 1" fill="currentColor">
          <rect width="10" height="1" />
        </svg>
      </button>
      <button
        onClick={maximize}
        className="inline-flex items-center justify-center w-12 h-full transition-colors"
        style={{ color: c.muted }}
        onMouseEnter={(e) => { e.currentTarget.style.backgroundColor = c.border; e.currentTarget.style.color = c.fg; }}
        onMouseLeave={(e) => { e.currentTarget.style.backgroundColor = "transparent"; e.currentTarget.style.color = c.muted; }}
        title="Maximize"
      >
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1">
          <rect x="0.5" y="0.5" width="9" height="9" />
        </svg>
      </button>
      <button
        onClick={close}
        className="inline-flex items-center justify-center w-12 h-full transition-colors"
        style={{ color: c.muted }}
        onMouseEnter={(e) => { e.currentTarget.style.backgroundColor = "#e81123"; e.currentTarget.style.color = "#fff"; }}
        onMouseLeave={(e) => { e.currentTarget.style.backgroundColor = "transparent"; e.currentTarget.style.color = c.muted; }}
        title="Close"
      >
        <svg width="10" height="10" viewBox="0 0 10 10" fill="currentColor">
          <path d="M1.41 0L5 3.59 8.59 0 10 1.41 6.41 5 10 8.59 8.59 10 5 6.41 1.41 10 0 8.59 3.59 5 0 1.41Z" />
        </svg>
      </button>
    </div>
  );
}

// ── Utils ───────────────────────────────────────────────────────────

function formatName(id: string): string {
  return id.split("-").map((w) => w.charAt(0).toUpperCase() + w.slice(1)).join(" ");
}
