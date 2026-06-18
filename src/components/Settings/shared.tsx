import { useState, type ReactNode } from "react";
import { CheckIcon, CopyIcon } from "../Icons";
import { isMac } from "../../lib/keybindings";
import type { LucideIcon } from "lucide-react";
import { UI_PANEL } from "../shared/design";

function testIdPart(value: string): string {
  return value.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
}

export function Group({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div data-testid="settings-group" data-settings-group={testIdPart(title)}>
      <h3 className="text-[var(--ct-muted-fg)] text-sm font-medium mb-2">{title}</h3>
      <div className="space-y-2.5">{children}</div>
    </div>
  );
}

export function SettingsPage({
  title,
  subtitle,
  children,
}: {
  title: string;
  subtitle?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="max-w-[720px] mx-auto flex flex-col gap-5" data-testid="settings-page" data-settings-page={testIdPart(title)}>
      {/* Title already shown in the nav header — keep it for screen readers only, no visual duplicate. */}
      <h1 className="sr-only">{title}</h1>
      {subtitle && (
        <p className="text-sm text-[var(--ct-muted-fg)] -mt-1">{subtitle}</p>
      )}
      {children}
    </div>
  );
}

export function SettingsCard({
  icon,
  title,
  headerRight,
  description,
  children,
  className,
}: {
  icon?: ReactNode;
  title: ReactNode;
  headerRight?: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
  className?: string;
}) {
  return (
    <section
      className={`rounded-xl border border-[var(--ct-border)] bg-[var(--ct-bg)]/40 p-4 ${className ?? ""}`}
      data-testid="settings-card"
      data-settings-card={typeof title === "string" ? testIdPart(title) : undefined}
    >
      <div className="flex items-center justify-between gap-3">
        <div className="flex items-center gap-2 text-[var(--ct-fg)] text-sm font-medium min-w-0">
          {icon}
          <span className="truncate">{title}</span>
        </div>
        {headerRight && <div className="shrink-0">{headerRight}</div>}
      </div>
      {description && (
        <p className="text-xs text-[var(--ct-muted-fg)] mt-2 leading-relaxed">{description}</p>
      )}
      {children && <div className="mt-3 space-y-2.5">{children}</div>}
    </section>
  );
}

const PILL_TONES = {
  green: "bg-[var(--ct-green)]/15 text-[var(--ct-green)]",
  red: "bg-[var(--ct-red)]/15 text-[var(--ct-red)]",
  muted: "bg-[var(--ct-border)] text-[var(--ct-muted-fg)]",
  accent: "bg-[var(--ct-accent)]/15 text-[var(--ct-accent)]",
  amber: "bg-[var(--ct-yellow)]/15 text-[var(--ct-yellow)]",
} as const;

export function SettingsPill({
  children,
  tone = "muted",
}: {
  children: ReactNode;
  tone?: keyof typeof PILL_TONES;
}) {
  return (
    <span className={`${UI_PANEL.smallPill} ${PILL_TONES[tone]}`} data-testid="settings-pill">{children}</span>
  );
}

export function SettingsRow({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className={`flex items-center justify-between gap-2 ${className ?? ""}`} data-testid="settings-row">
      {children}
    </div>
  );
}

export function Label({ text, children }: { text: string; children: React.ReactNode }) {
  return (
    <label className="block" data-testid="settings-label" data-settings-label={testIdPart(text)}>
      <span className="block text-sm text-[var(--ct-muted-fg)] mb-1">{text}</span>
      {children}
    </label>
  );
}

export function Toggle({ label, checked, onChange }: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="flex items-center justify-between cursor-pointer" data-testid="settings-toggle-row" data-settings-toggle={testIdPart(label)}>
      <span className="text-xs text-[var(--ct-muted-fg)]">{label}</span>
      <button
        type="button"
        role="switch"
        data-testid="settings-toggle"
        data-settings-toggle={testIdPart(label)}
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
    <div className="flex items-center justify-between text-xs gap-2" data-testid="settings-copy-row" data-settings-copy-row={testIdPart(label)}>
      <span className="text-[var(--ct-muted)] shrink-0">{label}</span>
      <div className="flex items-center gap-1 min-w-0">
        <code className="bg-[var(--ct-bg)] px-1.5 py-0.5 rounded text-[var(--ct-fg)] text-[10px] truncate">{value}</code>
        <button
          data-testid="settings-copy-button"
          data-settings-copy-row={testIdPart(label)}
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
    <div className="flex items-center justify-between text-xs" data-testid="settings-info-row" data-settings-info-row={testIdPart(label)}>
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

export function AboutLink({ href, label, Icon }: { href: string; label: string; Icon: LucideIcon }) {
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

export function renderKbd(binding: string) {
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
            className={UI_PANEL.kbd}
          >
            {display}
          </kbd>
        );
      })}
    </span>
  );
}
