import { useState, type KeyboardEvent, type ReactNode } from "react";
import { Check, MessageSquarePlus, type LucideIcon } from "lucide-react";
import { BackPill } from "../../shared/BackPill";
import { formatCompact, formatExact } from "../../../lib/formatCompact";
import { CHANGE_STATUS, changeBar } from "../changesViewModel";
import type { RecordedFileChange } from "@codeterm/chat-core";
import { breadcrumbSegments } from "./pathTree";

function editable(target: EventTarget | null): boolean {
  return target instanceof HTMLElement && (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName));
}

/** One panel page: a sticky bar over its content; with `onBack`, Escape and Backspace return to the list. */
export function PanelPage({ bar, onBack, reader, children }: { bar?: ReactNode; onBack?: () => void; reader?: boolean; children: ReactNode }) {
  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (!onBack || event.defaultPrevented || editable(event.target)) return;
    if (event.code !== "Escape" && event.code !== "Backspace") return;
    event.preventDefault();
    event.stopPropagation();
    onBack();
  }
  return <div className="tab-panel__page" onKeyDown={onKeyDown}>
    {bar && <div className="tab-panel__bar">{bar}</div>}
    <div className={`tab-panel__content${reader ? " tab-panel__content--reader" : ""}`}>{children}</div>
  </div>;
}

export function BackBar({ path, onBack, actions }: { path: string; onBack: () => void; actions?: ReactNode }) {
  const segments = breadcrumbSegments(path);
  const name = segments[segments.length - 1] ?? path;
  return <>
    <div className="tab-panel__bar-main">
      <BackPill label="list" onBack={onBack} />
      <nav aria-label="Path" className="tab-panel__crumbs" title={path}>
        <span className="tab-panel__crumb" aria-current="page">{name}</span>
      </nav>
    </div>
    {actions && <div className="tab-panel__bar-actions">{actions}</div>}
  </>;
}

export function IconAction({ label, icon: Icon, onClick, disabled, title }: { label: string; icon: LucideIcon; onClick: () => void; disabled?: boolean; title?: string }) {
  return <button type="button" className="tab-panel__icon-action" aria-label={label} title={title ?? label} disabled={disabled} onClick={onClick}>
    <Icon size={14} aria-hidden />
  </button>;
}

/** Shows a check after copying until the pointer or focus leaves; no timer decides when it resets. */
export function CopyAction({ label, icon: Icon, copy, disabled }: { label: string; icon: LucideIcon; copy: () => Promise<void>; disabled?: boolean }) {
  const [done, setDone] = useState(false);
  const Shown = done ? Check : Icon;
  return <button type="button" className="tab-panel__icon-action" aria-label={label} title={done ? "Copied" : label} disabled={disabled} data-done={done ? "" : undefined}
    onClick={() => void copy().then(() => setDone(true), () => setDone(false))} onMouseLeave={() => setDone(false)} onBlur={() => setDone(false)}>
    <Shown size={14} aria-hidden />
    <span className="sr-only" role="status">{done ? "Copied" : ""}</span>
  </button>;
}

export function ChatAction({ label, text, onClick, compact }: { label: string; text: string; onClick: () => void; compact?: boolean }) {
  return <button type="button" className={`tab-panel__chat-action${compact ? " tab-panel__chat-action--compact" : ""}`} aria-label={label} title={label} onClick={onClick}>
    <MessageSquarePlus size={compact ? 11 : 13} aria-hidden />{text}
  </button>;
}

export function PanelEmpty({ icon: Icon, children }: { icon: LucideIcon; children: ReactNode }) {
  return <div className="tab-panel__empty">
    <span className="tab-panel__empty-icon"><Icon size={18} aria-hidden /></span>
    <p className="m-0">{children}</p>
  </div>;
}

export function CompactCount({ value, unit }: { value: number; unit?: string }) {
  return <span title={unit ? `${formatExact(value)} ${unit}` : formatExact(value)}>{formatCompact(value)}</span>;
}

export function CountPair({ additions, deletions }: { additions: number; deletions: number }) {
  return <span className="tab-panel__counts" title={`${formatExact(additions)} added, ${formatExact(deletions)} removed lines`}>
    <span data-tone={additions ? "add" : "zero"}>+{formatCompact(additions)}</span>
    <span data-tone={deletions ? "del" : "zero"}>-{formatCompact(deletions)}</span>
  </span>;
}

export function ChangeBar({ additions, deletions }: { additions: number; deletions: number }) {
  return <span className="tab-panel__bar-chart" aria-hidden>
    {changeBar(additions, deletions).map((tone, index) => <span key={index} data-tone={tone} />)}
  </span>;
}

export function StatusGlyph({ kind }: { kind: RecordedFileChange["kind"] }) {
  const status = CHANGE_STATUS[kind];
  return <span className="tab-panel__status" data-status={kind} title={status.label}>
    <span aria-hidden>{status.glyph}</span><span className="sr-only">{status.label}</span>
  </span>;
}

export async function copyText(text: string) {
  if (!navigator.clipboard) throw new Error("Clipboard is unavailable");
  await navigator.clipboard.writeText(text);
}
