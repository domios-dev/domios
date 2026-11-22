import { useId } from "react";
import { Check } from "lucide-react";
import type { PickerSection } from "../../lib/pickerSettings";
import { DropdownSelect } from "../shared/DropdownMenu";
import { Segmented } from "../shared/Segmented";

export function PickerSettingSection({ section, busy, onChange }: {
  section: PickerSection;
  busy: boolean;
  onChange: (value: string) => void;
}) {
  const descriptionId = useId();
  if (section.kind === "boolean") {
    const checked = section.current === "true";
    return (
      <button
        type="button"
        role="checkbox"
        aria-checked={checked}
        aria-describedby={section.description ? descriptionId : undefined}
        disabled={busy}
        title={section.description ? `${section.label} — ${section.description}` : section.label}
        className="ct-menu-row ct-model-picker__boolean"
        onClick={() => { if (!busy) onChange(checked ? "false" : "true"); }}
      >
        <span className="min-w-0 flex-1 text-left">
          <span className="block truncate">{section.label}</span>
          {section.description && (
            <span id={descriptionId} className="ct-menu-row__description">{section.description}</span>
          )}
        </span>
        <span aria-hidden="true" className="ct-model-picker__checkbox" data-checked={checked ? "true" : undefined}>
          {checked && <Check size={11} strokeWidth={2.5} />}
        </span>
      </button>
    );
  }
  const options = section.options.some(option => option.value === section.current)
    ? section.options : [{ value: section.current, label: section.current || "Unknown" }, ...section.options];
  if (section.category === "effort") return <div className="ct-model-picker__effort">
    <span className="ct-model-picker__label">Reasoning</span>
    <Segmented aria-label="Reasoning" value={section.current}
      options={options.map(option => ({ ...option, disabled: busy || option.disabled }))} onChange={onChange} />
  </div>;
  return <div className="ct-model-picker__setting">
    <div className="ct-model-picker__setting-label">
      <span className="ct-model-picker__label" title={section.description ? `${section.label} — ${section.description}` : section.label}>{section.label}</span>
      {section.description && <span id={descriptionId} className="ct-menu-row__description">{section.description}</span>}
    </div>
    {section.options.length ? <DropdownSelect aria-label={section.label} aria-describedby={section.description ? descriptionId : undefined} disabled={busy} value={section.current}
      options={[...options]} onChange={onChange} className="min-w-0 !py-1" />
      : <output aria-label={section.label} className="pl-3">{section.current}</output>}
  </div>;
}
