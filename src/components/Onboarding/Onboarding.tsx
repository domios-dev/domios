import { useState, useEffect, useCallback, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { AppConfig, ThemeConfig } from "../../lib/types";

interface OnboardingProps {
  config: AppConfig;
  theme: ThemeConfig;
  onComplete: (config: AppConfig) => void;
}

const TOTAL_STEPS = 4;

export function Onboarding({ config, theme, onComplete }: OnboardingProps) {
  const [step, setStep] = useState(0);
  const [entered, setEntered] = useState(false);
  const [leaving, setLeaving] = useState(false);
  const [themes, setThemes] = useState<string[]>([]);
  const [selectedTheme, setSelectedTheme] = useState(theme.name);
  const [skillsInstalled, setSkillsInstalled] = useState(false);
  const [hooksInstalled, setHooksInstalled] = useState(false);
  const [rulesInstalled, setRulesInstalled] = useState(false);

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
    setTimeout(() => onComplete({ ...cfg, onboarding_completed: true }), 400);
  }, [onComplete]);

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
    try { await invoke("install_claude_hooks", { paneId: null }); setHooksInstalled(true); } catch {}
  }, []);
  const installRules = useCallback(async () => {
    try { await invoke("install_project_rules"); setRulesInstalled(true); } catch {}
  }, []);

  // Derived
  const progress = ((step + 1) / TOTAL_STEPS) * 100;
  const opacity = entered && !leaving ? 1 : 0;

  // Theme-derived colors
  const c = useMemo(() => ({
    bg: theme.background,
    fg: theme.foreground,
    muted: theme.bright_black,
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
          {/* Left: progress */}
          <div className="flex items-center gap-3 pl-4" data-tauri-drag-region>
            <span
              className="text-[11px] font-mono tracking-wider uppercase"
              style={{ color: c.muted }}
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

          {/* Window controls */}
          <WindowControls c={c} />
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
            {step === 3 && <StepFinish c={c} />}
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
            style={{ color: c.muted }}
            onMouseEnter={(e) => (e.currentTarget.style.color = c.fg)}
            onMouseLeave={(e) => (e.currentTarget.style.color = c.muted)}
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

type C = { bg: string; fg: string; muted: string; accent: string; green: string; surface: string; border: string; hover: string; glow: string };

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
        <span className="text-2xl font-mono font-bold" style={{ color: c.accent }}>
          {"</>"}
        </span>
      </div>

      <h1 className="text-2xl font-semibold mb-2" style={{ color: c.fg }}>
        Welcome to CodeTerm
      </h1>
      <p className="text-sm mb-10" style={{ color: c.muted }}>
        AI-first terminal multiplexer for developers
      </p>

      <div className="grid grid-cols-2 gap-3 text-left max-w-md mx-auto">
        <FeatureCard c={c} icon="+" title="Split panes" desc="Organize with flexible workspace layouts" />
        <FeatureCard c={c} icon="*" title="Agent orchestration" desc="Conductor/worker hierarchy for AI agents" />
        <FeatureCard c={c} icon="~" title="Claude integration" desc="Skills, hooks, auto-resume, notifications" />
        <FeatureCard c={c} icon=">" title="Browser panels" desc="Docs and dashboards inline with terminals" />
      </div>

      <p className="text-[11px] mt-8" style={{ color: c.muted }}>
        Just 4 quick steps. Everything is also in Settings (Ctrl+,).
      </p>
    </div>
  );
}

function FeatureCard({ c, icon, title, desc }: { c: C; icon: string; title: string; desc: string }) {
  return (
    <div
      className="px-4 py-3 rounded-lg"
      style={{ backgroundColor: c.surface + "30" }}
    >
      <div className="flex items-center gap-2 mb-1">
        <span className="text-xs font-mono" style={{ color: c.accent }}>{icon}</span>
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
        <span className="ml-2 text-[10px]" style={{ color: c.muted }}>Terminal</span>
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
          desc="Teaches Claude to control panes, workspaces, and agents"
          installed={skillsInstalled} onInstall={onInstallSkill}
        />
        <SetupRow c={c}
          title="Notification hooks"
          desc="Alerts when Claude finishes tasks or needs permission"
          installed={hooksInstalled} onInstall={onInstallHooks}
        />
        <SetupRow c={c}
          title="Project rules"
          desc="Tells Claude it's inside CodeTerm for optimal behavior"
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

// ── Step 4: Done ────────────────────────────────────────────────────

function StepFinish({ c }: { c: C }) {
  return (
    <div className="text-center">
      <div
        className="inline-flex items-center justify-center w-14 h-14 rounded-full mb-6"
        style={{ backgroundColor: c.green + "15" }}
      >
        <span className="text-2xl" style={{ color: c.green }}>&#10003;</span>
      </div>

      <h2 className="text-xl font-semibold mb-1" style={{ color: c.fg }}>
        Ready to go
      </h2>
      <p className="text-sm mb-8" style={{ color: c.muted }}>
        Start Claude Code in any pane — CodeTerm auto-detects it.
      </p>

      <div className="max-w-xs mx-auto space-y-1.5">
        <ShortcutRow c={c} keys="Ctrl+Shift+T" action="New tab" />
        <ShortcutRow c={c} keys="Ctrl+Shift+D" action="Split right" />
        <ShortcutRow c={c} keys="Ctrl+Shift+E" action="Split down" />
        <ShortcutRow c={c} keys="Ctrl+Shift+N" action="New workspace" />
        <ShortcutRow c={c} keys="Ctrl+," action="Settings" />
      </div>

      <div
        className="mt-8 mx-auto max-w-sm px-4 py-3 rounded-lg border text-[11px] text-left"
        style={{ borderColor: c.border, backgroundColor: c.surface + "60", color: c.muted }}
      >
        <span style={{ color: c.accent }}>Tip:</span> Type{" "}
        <code
          className="px-1 py-0.5 rounded text-[10px] font-mono"
          style={{ backgroundColor: c.border, color: c.fg }}
        >
          claude
        </code>{" "}
        in a terminal to start your AI coding assistant. It will automatically connect to CodeTerm.
      </div>
    </div>
  );
}

function ShortcutRow({ c, keys, action }: { c: C; keys: string; action: string }) {
  return (
    <div className="flex items-center justify-between text-xs py-1">
      <span style={{ color: c.muted }}>{action}</span>
      <kbd
        className="px-2 py-0.5 rounded text-[11px] font-mono"
        style={{ backgroundColor: c.border, color: c.fg }}
      >
        {keys}
      </kbd>
    </div>
  );
}

// ── Window controls ─────────────────────────────────────────────────

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
