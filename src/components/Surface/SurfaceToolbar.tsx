import { useEffect, useRef, type KeyboardEvent } from "react";
import type { ResolvedToolbarItem } from "./surfaceChromeModel";
import { surfaceIcon } from "./surfaceIcons";

export interface SurfaceToolbarProps {
  label: string;
  items: ResolvedToolbarItem[];
  onButton(id: string): void;
  onToggle(id: string): void;
  onSubmit(id: string, value: string): void;
  onSelect(id: string, value: string): void;
}

const ICON_SIZE = 16;

function IconOrLabel({ icon, label }: { icon?: string; label: string }) {
  const Icon = surfaceIcon(icon);
  return Icon ? <Icon size={ICON_SIZE} aria-hidden="true" /> : <span>{label}</span>;
}

// Uncontrolled so a page navigating underneath never overwrites what the user is typing.
function ToolbarInput({ item, onSubmit }: { item: Extract<ResolvedToolbarItem, { kind: "input" }>; onSubmit: (value: string) => void }) {
  const ref = useRef<HTMLInputElement | null>(null);
  useEffect(() => {
    const input = ref.current;
    if (input && input.ownerDocument.activeElement !== input) input.value = item.value;
  }, [item.value]);
  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    const input = event.currentTarget;
    if (event.key === "Enter") {
      event.preventDefault();
      onSubmit(input.value);
      input.blur();
    } else if (event.key === "Escape" && !event.shiftKey) {
      event.preventDefault();
      event.stopPropagation();
      input.value = item.value;
      input.blur();
    }
  };
  return (
    <input
      ref={ref}
      type="text"
      className="ct-surface-pill ct-surface-address"
      aria-label={item.label}
      placeholder={item.placeholder}
      spellCheck={false}
      autoComplete="off"
      disabled={item.disabled}
      defaultValue={item.value}
      onBlur={(event) => { event.currentTarget.value = item.value; }}
      onKeyDown={onKeyDown}
    />
  );
}

export function SurfaceToolbar({ label, items, onButton, onToggle, onSubmit, onSelect }: SurfaceToolbarProps) {
  return (
    <div role="toolbar" aria-label={`${label} controls`} className="ct-surface-toolbar ct-jobs-header">
      {items.map((item) => {
        switch (item.kind) {
          case "input":
            return <ToolbarInput key={item.id} item={item} onSubmit={(value) => onSubmit(item.id, value)} />;
          case "select":
            return (
              <select
                key={item.id}
                className="ct-surface-pill ct-surface-select ct-liquid-interactive"
                aria-label={item.label}
                title={item.label}
                disabled={item.disabled}
                value={item.value}
                onChange={(event) => onSelect(item.id, event.target.value)}
              >
                {item.options.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
              </select>
            );
          case "toggle":
            return (
              <button
                key={item.id}
                type="button"
                className={`${item.icon ? "ct-surface-icon-btn" : "ct-surface-pill ct-surface-take"} ct-liquid-interactive`}
                aria-label={item.label}
                aria-pressed={item.pressed}
                title={item.label}
                disabled={item.disabled}
                onClick={() => onToggle(item.id)}
              >
                <IconOrLabel icon={item.icon} label={item.label} />
              </button>
            );
          case "button":
            return (
              <button
                key={item.id}
                type="button"
                className="ct-surface-icon-btn ct-liquid-interactive"
                aria-label={item.label}
                title={item.label}
                disabled={item.disabled}
                onClick={() => onButton(item.id)}
              >
                <IconOrLabel icon={item.icon} label={item.label} />
              </button>
            );
        }
      })}
    </div>
  );
}
