import { useId, useState, type KeyboardEvent, type ReactNode } from "react";
import { cleanMachineName, MACHINE_COLORS, MACHINE_KINDS, type MachineIdentity, type MachineKind } from "../../lib/machineIdentity";
import { CursorTooltip } from "../shared/CursorTooltip";
import { MachineGlyph } from "../shared/MachineGlyph";

const kinds = Object.keys(MACHINE_KINDS) as MachineKind[];

export function machineColourLabel(colour: MachineIdentity["color"]): string {
  return colour[0].toUpperCase() + colour.slice(1);
}

function RadioChoices<T extends string>({ label, values, value, onChange, children }: {
  label: string;
  values: readonly T[];
  value: T;
  onChange: (choice: T) => void;
  children: (option: T) => { label: string; body: ReactNode };
}) {
  const id = useId();
  function move(event: KeyboardEvent<HTMLDivElement>) {
    const buttons = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="radio"]'));
    const current = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (current < 0) return;
    const offset = event.key === "ArrowRight" || event.key === "ArrowDown" ? 1 : event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 0;
    const next = offset ? (current + offset + values.length) % values.length : event.key === "Home" ? 0 : event.key === "End" ? values.length - 1 : -1;
    if (next < 0) return;
    event.preventDefault();
    buttons[next]?.focus();
    onChange(values[next]);
  }

  return <div className="machine-options">
    <div><span id={id}>{label}</span> <span className="text-[var(--ct-fg)]">{children(value).label}</span></div>
    <div role="radiogroup" aria-labelledby={id} onKeyDown={move} className="machine-options__group">
      {values.map(option => {
        const { label, body } = children(option);
        return <CursorTooltip key={option} as="span" content={label} active>
          <button type="button" role="radio" aria-checked={option === value} aria-label={label}
            tabIndex={option === value ? 0 : -1} onClick={() => onChange(option)}>
            {body}
          </button>
        </CursorTooltip>;
      })}
    </div>
  </div>;
}

export function MachineIdentityControls({ value, onChange }: { value: MachineIdentity; onChange: (identity: MachineIdentity) => void }) {
  return <div className="machine-options-stack">
    <RadioChoices label="Kind" values={kinds} value={value.kind} onChange={kind => onChange({ ...value, kind })}>
      {kind => ({
        label: MACHINE_KINDS[kind].label,
        body: <MachineGlyph kind={kind} color={value.color} size="mark-15" muted={kind !== value.kind} />,
      })}
    </RadioChoices>
    <RadioChoices label="Colour" values={MACHINE_COLORS} value={value.color} onChange={color => onChange({ ...value, color })}>
      {color => ({ label: machineColourLabel(color), body: <span className="machine-options__swatch" style={{ color: `var(--ct-machine-${color})` }} /> })}
    </RadioChoices>
  </div>;
}

export function MachineJoinPreview({ name, value, onChange, initiallyExpanded = false }: {
  name: string;
  value: MachineIdentity;
  onChange: (identity: MachineIdentity) => void;
  initiallyExpanded?: boolean;
}) {
  const controlsId = useId();
  const displayName = cleanMachineName(name) ?? "This machine";
  const [expanded, setExpanded] = useState(initiallyExpanded);
  return <div className="flex w-full flex-col gap-2" aria-label="What is this machine?" role="group">
    <div className="flex items-center gap-2">
      <MachineGlyph {...value} size="tile" />
      <div className="min-w-0 flex-1">
        <div className="truncate" title={displayName}>{displayName}</div>
        <div className="text-[length:var(--ct-font-size-meta)] text-[var(--ct-muted-fg)]">{MACHINE_KINDS[value.kind].label} · joins your mesh</div>
      </div>
      <button type="button" className="ct-menu-row machine-identity-action shrink-0" aria-controls={controlsId}
        aria-expanded={expanded} onClick={() => setExpanded(value => !value)}>{expanded ? "Done" : "Change"}</button>
    </div>
    <div id={controlsId} hidden={!expanded}>
      <MachineIdentityControls value={value} onChange={onChange} />
    </div>
  </div>;
}

export function MachineIdentitySaveError({ error, retry, title = "Couldn't save the icon and colour." }: { error: string; retry: () => void; title?: string }) {
  return <div className="flex flex-wrap items-baseline gap-x-3 border-t border-[var(--ct-border)] pt-2 text-[length:var(--ct-font-size-meta)]">
    <span role="status" className="w-full">{title}</span>
    <button type="button" className="machine-identity-action font-semibold text-[var(--ct-accent)]" onClick={retry}>Try again</button>
    <details className="min-w-0 flex-auto">
      <summary>Details</summary>
      <p className="break-words">{error}</p>
    </details>
  </div>;
}
