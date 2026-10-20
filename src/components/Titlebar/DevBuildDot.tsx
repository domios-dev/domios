import { useId, useRef, useState } from "react";
import { AnchoredPopover } from "../shared/AnchoredPopover";

/** A quiet dot that marks a development build; its label opens on hover or focus, like a message's dot. */
export function DevBuildDot({ label, version }: { label: string; version: string }) {
  const [open, setOpen] = useState(false);
  const labelId = useId();
  const triggerRef = useRef<HTMLButtonElement>(null);
  return (
    <span
      className="ct-dev-dot"
      onPointerEnter={(event) => { if (event.pointerType === "mouse") setOpen(true); }}
      onPointerLeave={() => setOpen(false)}
    >
      <button
        ref={triggerRef}
        type="button"
        className="ct-dev-dot__button"
        aria-label={`Development build, version ${version}`}
        aria-describedby={open ? labelId : undefined}
        onFocus={() => setOpen(true)}
        onBlur={() => setOpen(false)}
        onClick={() => setOpen((current) => !current)}
      >
        <span aria-hidden="true" className="ct-dev-dot__mark" />
      </button>
      <AnchoredPopover open={open} anchor={triggerRef} placement="bottom-start" gap={6} onClose={() => setOpen(false)}>
        <span id={labelId} role="tooltip" className="ct-menu ct-dev-dot__label">{label}</span>
      </AnchoredPopover>
    </span>
  );
}
