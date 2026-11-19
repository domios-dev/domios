import type { ReactNode } from "react";
import { DropdownMenu, DropdownSelect, type DropdownMenuItem, type DropdownSelectOption } from "./DropdownMenu";
import { MachineGlyph } from "./MachineGlyph";
import { machineLabel } from "../../lib/machineIdentity";
import { LOCAL_MACHINE_ID, type MachinePlacementChoice } from "../../lib/machinePlacement";

export function MachinePlacementChoiceContent({ choice }: { choice: MachinePlacementChoice }) {
  const statusColor = choice.status === "online" ? "var(--ct-green)"
    : choice.status === "needs update" ? "var(--ct-yellow)" : "var(--ct-muted)";
  return <span className="flex w-full min-w-0 items-center gap-2">
    <MachineGlyph kind={choice.identity?.kind} color={choice.identity?.color} size="mark-15" />
    <span className="min-w-0 flex-1 truncate">{choice.name}</span>
    <span className="inline-flex shrink-0 items-center gap-1.5 text-[var(--ct-muted-fg)]">
      <span aria-hidden="true" className="size-2 rounded-full" style={{ backgroundColor: statusColor }} />
      {choice.status}
    </span>
  </span>;
}

function choiceOption(choice: MachinePlacementChoice): DropdownSelectOption {
  return {
    value: choice.id,
    label: choice.name,
    ariaLabel: machineLabel(choice.name, choice.identity, choice.status),
    unavailable: choice.unavailableReason !== null,
    description: choice.unavailableReason ?? undefined,
    content: <MachinePlacementChoiceContent choice={choice} />,
  };
}

export function MachinePlacementMenu({ choices, value, onChange, trigger, menuLabel, header, disabled = false, disabledLocal = false, testId }: {
  choices: readonly MachinePlacementChoice[] | null;
  value: string;
  onChange: (machineId: string) => void;
  trigger: ReactNode;
  menuLabel: string;
  header?: ReactNode;
  disabled?: boolean;
  disabledLocal?: boolean;
  testId?: string;
}) {
  if (!choices || choices.length <= 1) return null;
  const items: DropdownMenuItem[] = choices.map((choice) => ({
    label: choice.name,
    "aria-label": machineLabel(choice.name, choice.identity, choice.status),
    labelContent: <MachinePlacementChoiceContent choice={choice} />,
    description: choice.unavailableReason ?? undefined,
    unavailable: choice.unavailableReason !== null,
    disabled: choice.local && disabledLocal,
    selected: choice.id === value,
    onClick: () => onChange(choice.id),
    "data-testid": testId ? `${testId}-option-${choice.id}` : undefined,
  }));
  return <DropdownMenu items={items} trigger={trigger} menuLabel={menuLabel} header={header} disabled={disabled}
    menuTestId={testId ? `${testId}-menu` : undefined} placement="bottom-end" />;
}

export function MachinePlacement({ choices, value, onChange, disabled = false, testId, labelWidth = "100px" }: {
  choices: readonly MachinePlacementChoice[] | null;
  value: string;
  onChange: (machineId: string) => void;
  disabled?: boolean;
  testId?: string;
  labelWidth?: "100px" | "140px";
}) {
  if (!choices || (choices.length <= 1 && (!value || value === LOCAL_MACHINE_ID))) return null;
  const options = choices.map(choiceOption);
  if (value && !options.some((option) => option.value === value)) {
    options.unshift({ value, label: "Unavailable machine", disabled: true, description: "This machine is no longer paired." });
  }
  return <div className={`grid min-w-0 items-center gap-2 text-[12px] ${labelWidth === "140px" ? "grid-cols-[140px_minmax(0,176px)]" : "grid-cols-[100px_minmax(0,1fr)]"}`}>
    <span className="truncate whitespace-nowrap">Where it runs</span>
    <DropdownSelect
      aria-label="Where it runs"
      data-testid={testId}
      options={options}
      value={value || LOCAL_MACHINE_ID}
      onChange={onChange}
      disabled={disabled}
      placeholder="Choose a machine"
      selectionIndicator="highlight"
    />
  </div>;
}
