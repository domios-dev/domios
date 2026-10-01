import { useId, useState, type ReactNode } from "react";
import { CheckIcon, CopyIcon } from "../Icons";
import { isMac } from "../../lib/keybindings";
import type { LucideIcon } from "lucide-react";
import { UI_PANEL } from "../shared/design";
import { Button } from "../shared/Button";
import { Card } from "../shared/Card";
import { Chip, type ChipVariant } from "../shared/Chip";
import { FormRow } from "../shared/FormRow";
import { LiquidToggle, LiquidToggleTrack, useLiquidTravel } from "../shared/LiquidToggle";

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
  const titleId = useId();
  return (
    <div
      role="region"
      aria-labelledby={titleId}
      className="max-w-[720px] mx-auto flex flex-col gap-5"
      data-testid="settings-page"
      data-settings-page={testIdPart(title)}
    >
      {/* Title already shown in the nav header — keep it for screen readers only, no visual duplicate. */}
      <h1 id={titleId} className="sr-only">{title}</h1>
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
  const titleId = useId();
  return (
    <Card
      role="group"
      aria-labelledby={titleId}
      className={`p-4 ${className ?? ""}`}
      data-testid="settings-card"
    >
      <div className="flex items-center justify-between gap-3">
        <div className="flex items-center gap-2 text-[var(--ct-fg)] text-sm font-medium min-w-0">
          {icon}
          <span id={titleId} className="truncate">{title}</span>
        </div>
        {headerRight && <div className="shrink-0">{headerRight}</div>}
      </div>
      {description && (
        <p className="text-xs text-[var(--ct-muted-fg)] mt-2 leading-relaxed">{description}</p>
      )}
      {children && <div className="mt-3 space-y-2.5">{children}</div>}
    </Card>
  );
}

const PILL_TONES: Record<string, ChipVariant> = {
  green: "success",
  red: "danger",
  muted: "muted",
  accent: "accent",
  amber: "warning",
} as const;

export function SettingsPill({
  children,
  tone = "muted",
}: {
  children: ReactNode;
  tone?: keyof typeof PILL_TONES;
}) {
  return (
    <Chip variant={PILL_TONES[tone]} data-testid="settings-pill">{children}</Chip>
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
    <FormRow label={text} data-testid="settings-label">{children}</FormRow>
  );
}

export function Toggle({ label, checked, onChange }: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="flex items-center justify-between cursor-pointer" data-testid="settings-toggle-row" data-settings-toggle={testIdPart(label)}>
      <span className="text-xs text-[var(--ct-muted-fg)]">{label}</span>
      <LiquidToggle
        // A role="switch" button is not labelable, so the wrapping <label> never named it.
        aria-label={label}
        data-testid="settings-toggle"
        data-settings-toggle={testIdPart(label)}
        checked={checked}
        onCheckedChange={onChange}
      />
    </label>
  );
}

/**
 * One answerable question, with its own header and hairline. A panel that asks
 * several unrelated things is several sections rather than one flat list.
 */
export function SettingsSection({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  const titleId = useId();
  return (
    <section
      aria-labelledby={titleId}
      className="ct-settings-section"
      data-testid="settings-section"
      data-settings-section={testIdPart(title)}
    >
      <div className="ct-settings-section__header">
        <span id={titleId} className="ct-settings-section__label">{title}</span>
        <span aria-hidden className="ct-settings-section__rule" />
      </div>
      {children}
    </section>
  );
}

/**
 * A switch and the words it governs, as ONE control.
 *
 * The tile is the switch: it carries the role, the state and the name, and the
 * liquid track inside it is decorative. That is what keeps the two attached —
 * a row with the control pushed to the far edge reads as a switch that belongs
 * to nothing, and a tile that rendered its own focusable switch would meet a
 * reader twice. The hint is deliberately outside the accessible name: it
 * explains the answer, it is not the question.
 */
export function SwitchTile({
  label,
  hint,
  checked,
  onChange,
  dense,
}: {
  label: string;
  hint?: ReactNode;
  checked: boolean;
  onChange: (value: boolean) => void;
  dense?: boolean;
}) {
  const { flip, travel, finishTravel } = useLiquidTravel(checked, onChange);
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={flip}
      className={`ct-switch-tile${dense ? " ct-switch-tile--dense" : ""}`}
      data-testid="settings-switch-tile"
      data-settings-toggle={testIdPart(label)}
    >
      <LiquidToggleTrack checked={checked} travel={travel} onTravelEnd={finishTravel} />
      <span className="flex min-w-0 flex-col gap-0.5">
        <span className="text-xs text-[var(--ct-fg)]">{label}</span>
        {hint && (
          <span className="text-[10px] leading-[1.4] text-[var(--ct-muted)]">{hint}</span>
        )}
      </span>
    </button>
  );
}

export function CopyRow({ label, value }: { label: string; value: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <div className="flex items-center justify-between text-xs gap-2" data-testid="settings-copy-row" data-settings-copy-row={testIdPart(label)}>
      <span className="text-[var(--ct-muted)] shrink-0">{label}</span>
      <div className="flex items-center gap-1 min-w-0">
        <code className="bg-[var(--ct-bg)] px-1.5 py-0.5 rounded text-[var(--ct-fg)] text-[10px] truncate">{value}</code>
        <Button
          variant="icon"
          size="compact"
          data-testid="settings-copy-button"
          data-settings-copy-row={testIdPart(label)}
          onClick={() => {
            navigator.clipboard.writeText(value).then(() => {
              setCopied(true);
              setTimeout(() => setCopied(false), 1500);
            });
          }}
          className="shrink-0 !w-5 !h-5 !min-h-5 text-[var(--ct-muted)]"
          title="Copy"
        >
          {copied ? (
            <CheckIcon />
          ) : (
            <CopyIcon />
          )}
        </Button>
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

/** An in-app navigation link: a button styled as a link, since it routes rather than loads a URL. */
export function InlineLinkButton({ label, onClick, testId }: { label: string; onClick: () => void; testId?: string }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="text-[11px] text-[var(--ct-accent)] underline underline-offset-2 hover:text-[var(--ct-fg)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ct-accent)]"
      data-testid={testId}
    >
      {label}
    </button>
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
