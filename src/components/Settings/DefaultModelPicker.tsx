import { useRef, useState } from "react";
import { ChevronDown } from "lucide-react";
import { useAgentDescriptor } from "../../lib/useAgentDescriptor";
import { ModelPickerPopover } from "../Terminal/StatusLineModelPicker";
import { currentModelId } from "../Terminal/statusLinePickerModel";

export function DefaultModelPicker({ providerId, value, onChange }: {
  providerId: string;
  value: string;
  onChange: (model: string) => void;
}) {
  const descriptor = useAgentDescriptor(providerId);
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const current = currentModelId(descriptor, [value]);
  const label = descriptor?.available_models.find((model) => model.id === current)?.display_name ?? value;
  const close = () => {
    setOpen(false);
    trigger.current?.focus({ preventScroll: true });
  };
  return <div className="min-w-0">
    <button ref={trigger} type="button" className="ct-bar-pill max-w-full" aria-label="Choose default model"
      aria-haspopup="dialog" aria-expanded={open} disabled={!descriptor} onClick={() => setOpen((previous) => !previous)}>
      <span className="min-w-0 truncate">{label || (descriptor ? "Provider default" : "Loading models…")}</span>
      <ChevronDown size={13} aria-hidden="true" />
    </button>
    {open && descriptor && <ModelPickerPopover descriptor={descriptor}
      gate={{ interactive: true, timing: "live", confirmRestart: false, note: "Used for new sessions" }}
      triggerRef={trigger} currentModel={current} currentEffort={undefined} showReasoning={false} busy={false} failure={null}
      onCommit={(selection) => { onChange(selection.model); close(); }} onClose={close} />}
  </div>;
}
