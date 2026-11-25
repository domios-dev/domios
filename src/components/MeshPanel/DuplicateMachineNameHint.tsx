import { useRef, useState } from "react";
import { UNKNOWN_MACHINE } from "../../lib/machineIdentity";
import { useMeshMachineIdentity } from "../../lib/useMeshMachineIdentities";
import { MachineIdentityPopover, type MachineRename } from "./MachineIdentityEditor";
import { UnavailableMachineRename } from "./UnavailableMachineRename";

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
  const renameLabel = `Rename ${name} in Machine icon`;
  return <p className={`m-0 text-[length:var(--ct-font-size-meta)] text-[var(--ct-muted-fg)] ${className}`} onClick={(event) => event.stopPropagation()}>
    {sameMachineNameDescription(others)}
    {" · "}
    {rename ? <button type="button" className="underline underline-offset-2 hover:text-[var(--ct-fg)]" ref={anchor}
      aria-label={renameLabel} aria-expanded={open} onClick={() => setOpen(true)}>Rename</button>
      : <UnavailableMachineRename label={renameLabel}>Rename</UnavailableMachineRename>}
    {open && rename && <MachineIdentityPopover name={name} initial={identity ?? UNKNOWN_MACHINE} peerId={peerId}
      anchor={anchor} focusName rename={rename}
      onClose={() => { setOpen(false); anchor.current?.focus({ preventScroll: true }); }} />}
  </p>;
}
