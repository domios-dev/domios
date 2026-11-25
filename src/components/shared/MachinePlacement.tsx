import type { ReactNode } from "react";
import { DropdownMenu, DropdownSelect, type DropdownMenuItem, type DropdownSelectOption } from "./DropdownMenu";
import { MachineGlyph } from "./MachineGlyph";
import { LOCAL_MACHINE_ID, type MachinePlacementChoice } from "../../lib/machinePlacement";
import { MachinePickerRow, MachineThisBadge, machineRowLabel } from "./MachineRow";

function machineRowProps(choice: MachinePlacementChoice) {
  return {
    name: choice.name,
    identity: choice.identity,
    status: choice.status,
    local: choice.local,
  };
}

function choiceOption(choice: MachinePlacementChoice): DropdownSelectOption {
  const row = machineRowProps(choice);
  return {
    value: choice.id,
    label: choice.name,
    ariaLabel: machineRowLabel(row),
    unavailable: choice.unavailableReason !== null,
    description: choice.unavailableReason ?? undefined,
    stretchLabelContent: true,
    content: <MachinePickerRow {...row} />,
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
  const items: DropdownMenuItem[] = choices.map((choice) => {
    const row = machineRowProps(choice);
    return {
      label: choice.name,
      "aria-label": machineRowLabel(row),
      stretchLabelContent: true,
      labelContent: <MachinePickerRow {...row} />,
      description: choice.unavailableReason ?? undefined,
      unavailable: choice.unavailableReason !== null,
      disabled: choice.local && disabledLocal,
      selected: choice.id === value,
      onClick: () => onChange(choice.id),
      "data-testid": testId ? `${testId}-option-${choice.id}` : undefined,
    };
  });
  return <DropdownMenu items={items} trigger={trigger} menuLabel={menuLabel} header={header} disabled={disabled}
    menuTestId={testId ? `${testId}-menu` : undefined} placement="bottom-end" opaque />;
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
  const selected = choices.find((choice) => choice.id === (value || LOCAL_MACHINE_ID));
  if (value && !options.some((option) => option.value === value)) {
    options.unshift({ value, label: "Unavailable machine", disabled: true, description: "This machine is no longer paired." });
  }
  return <div className={`grid min-w-0 items-center gap-2 text-[12px] ${labelWidth === "140px" ? "grid-cols-[140px_minmax(0,1fr)]" : "grid-cols-[100px_minmax(0,1fr)]"}`}>
    <span className="truncate whitespace-nowrap">Where it runs</span>
    <DropdownSelect
      aria-label="Where it runs"
      data-testid={testId}
      options={options}
      value={value || LOCAL_MACHINE_ID}
      onChange={onChange}
      disabled={disabled}
      placeholder="Choose a machine"
      leading={<MachineGlyph kind={selected?.identity?.kind} size="mark-15" />}
      trailing={selected?.local ? <MachineThisBadge /> : undefined}
      selectionIndicator="highlight"
    />
  </div>;
}
