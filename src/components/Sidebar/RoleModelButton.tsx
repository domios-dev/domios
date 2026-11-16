import { useRef, useState } from "react";
import { ChevronDown } from "lucide-react";
import type { ProviderDescriptor } from "../../lib/types";
import { reasoningEffortsForModel } from "../../lib/newSessionChoice";
import { ModelPickerPopover } from "../Terminal/StatusLineModelPicker";
import { SELECT_TRIGGER } from "../shared/DropdownMenu";

/** "Opus 5.5 · High": the model and its effort as one value. */
export function modelEffortText(provider: ProviderDescriptor | undefined, modelId: string, effortId?: string): string {
  const model = provider?.available_models.find((entry) => entry.id === modelId)?.display_name ?? modelId;
  const effort = effortId ? reasoningEffortsForModel(provider, modelId).find((entry) => entry.id === effortId)?.display_name ?? effortId : "";
  const effortText = effort ? `${effort.charAt(0).toUpperCase()}${effort.slice(1)}` : "";
  return [model || "Choose a model", effortText].filter(Boolean).join(" · ");
}

/** One compact model pill for a role row; it opens the same model picker as a running tab. A seat carries only model and effort, so Advanced stays hidden. */
export function RoleModelButton({ label, provider, modelId, effortId, disabled, onChange, testId }: {
  label: string;
  provider: ProviderDescriptor | undefined;
  modelId: string;
  effortId?: string;
  disabled?: boolean;
  onChange: (modelId: string, effortId?: string) => void;
  testId?: string;
}) {
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const text = modelEffortText(provider, modelId, effortId);
  const close = () => {
    setOpen(false);
    trigger.current?.focus({ preventScroll: true });
  };
  const unavailable = !provider || provider.available_models.length === 0;
  return <>
    <button ref={trigger} type="button" data-testid={testId} data-value={modelId} aria-label={`${label} model: ${text}`}
      aria-haspopup="dialog" aria-expanded={open} disabled={disabled || unavailable}
      className={SELECT_TRIGGER} onClick={() => setOpen((previous) => !previous)}>
      <span className="min-w-0 flex-1 truncate text-left">{unavailable ? "No models published" : text}</span>
      <ChevronDown size={12} aria-hidden="true" className="shrink-0 text-[var(--ct-muted-fg)]" />
    </button>
    {open && provider && <ModelPickerPopover descriptor={provider} showAdvanced={false}
      gate={{ interactive: true, timing: "live", confirmRestart: false, note: `Used when the ${label} starts` }}
      triggerRef={trigger} currentModel={modelId} currentEffort={effortId} busy={false} failure={null}
      onCommit={(selection) => { onChange(selection.model, selection.effort); close(); }} onClose={close} />}
  </>;
}
