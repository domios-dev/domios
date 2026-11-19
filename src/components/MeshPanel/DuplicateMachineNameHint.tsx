import { useId, useRef, useState } from "react";
import { UNKNOWN_MACHINE } from "../../lib/machineIdentity";
import { useMeshMachineIdentity } from "../../lib/useMeshMachineIdentities";
import { MachineIdentityPopover, type MachineRename } from "./MachineIdentityEditor";

const UNAVAILABLE = "Rename it on that machine, or when it's back online.";

export function sameMachineNameDescription(others: number): string {
  return `Same name as ${others} ${others === 1 ? "other" : "others"}`;
}

export function DuplicateMachineNameHint({ name, others, peerId, rename, className = "" }: {
  name: string;
  others?: number;
  peerId?: string;
  rename?: MachineRename;
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  const descriptionId = useId();
  const anchor = useRef<HTMLButtonElement>(null);
  const identity = useMeshMachineIdentity(peerId, open);
  if (!others) return null;
  return <p className={`m-0 text-[length:var(--ct-font-size-meta)] text-[var(--ct-muted-fg)] ${className}`} onClick={(event) => event.stopPropagation()}>
    {sameMachineNameDescription(others)}
    {" · "}
    <button type="button" className={`underline underline-offset-2 ${rename ? "hover:text-[var(--ct-fg)]" : "cursor-not-allowed"}`} ref={anchor}
      aria-label={`Rename ${name} in Icon and colour`} aria-expanded={rename ? open : undefined}
      aria-disabled={!rename || undefined} aria-describedby={rename ? undefined : descriptionId}
      title={rename ? undefined : UNAVAILABLE} onClick={() => { if (rename) setOpen(true); }}>Rename</button>
    {!rename && <span id={descriptionId} hidden>{UNAVAILABLE}</span>}
    {open && rename && <MachineIdentityPopover name={name} initial={identity ?? UNKNOWN_MACHINE} peerId={peerId}
      anchor={anchor} focusName rename={rename}
      onClose={() => { setOpen(false); anchor.current?.focus({ preventScroll: true }); }} />}
  </p>;
}
