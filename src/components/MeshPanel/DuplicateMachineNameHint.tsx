import { useRef, useState } from "react";
import { UNKNOWN_MACHINE } from "../../lib/machineIdentity";
import { useMeshMachineIdentity } from "../../lib/useMeshMachineIdentities";
import { UI_TEXT } from "../shared/design";
import { MachineIdentityPopover, type MachineRename } from "./MachineIdentityEditor";

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
  const anchor = useRef<HTMLButtonElement>(null);
  const identity = useMeshMachineIdentity(peerId, open);
  if (!others) return null;
  return <p className={`m-0 ${UI_TEXT.metaMuted} ${className}`} onClick={(event) => event.stopPropagation()}>
    {sameMachineNameDescription(others)}
    {rename && <>
      {" · "}
      <button type="button" className="underline underline-offset-2 hover:text-[var(--ct-fg)]" ref={anchor}
        aria-label={`Rename ${name} in Icon and colour`} aria-expanded={open} onClick={() => setOpen(true)}>Rename</button>
      {open && <MachineIdentityPopover name={name} initial={identity ?? UNKNOWN_MACHINE} peerId={peerId}
        anchor={anchor} focusName rename={rename}
        onClose={() => { setOpen(false); anchor.current?.focus({ preventScroll: true }); }} />}
    </>}
  </p>;
}
