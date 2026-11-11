import type { PickerSection } from "../../lib/pickerSettings";
import { DropdownSelect } from "../shared/DropdownMenu";
import { Segmented } from "../shared/Segmented";

export function PickerSettingSection({ section, busy, onChange }: {
  section: PickerSection;
  busy: boolean;
  onChange: (value: string) => void;
}) {
  const options = section.options.some(option => option.value === section.current)
    ? section.options : [{ value: section.current, label: section.current || "Unknown" }, ...section.options];
  if (section.category === "effort") return <div className="ct-model-picker__effort">
    <span className="ct-model-picker__label">Reasoning</span>
    <Segmented aria-label="Reasoning" value={section.current}
      options={options.map(option => ({ ...option, disabled: busy || option.disabled }))} onChange={onChange} />
  </div>;
  return <div className="ct-model-picker__setting">
    <span className="ct-model-picker__label" title={section.label}>{section.label}</span>
    {section.options.length ? <DropdownSelect aria-label={section.label} disabled={busy} value={section.current}
      options={[...options]} onChange={onChange} className="min-w-0 !py-1" />
      : <output aria-label={section.label} className="pl-3">{section.current}</output>}
  </div>;
}
