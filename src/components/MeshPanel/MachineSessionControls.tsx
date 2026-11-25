import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { UI_BUTTON, UI_TEXT } from "../shared/design";
import { DropdownMenu } from "../shared/DropdownMenu";
import { useMeshMachineIdentity } from "../../lib/useMeshMachineIdentities";
import { UNKNOWN_MACHINE } from "../../lib/machineIdentity";
import { sameMachineNameDescription } from "./DuplicateMachineNameHint";
import { MachineIdentityPopover, type MachineRename } from "./MachineIdentityEditor";

export function MachineSessionControls({ name, busy, online = false, onSignOut, onRemove, renderRow, identityPeer, rename, sameNameOthers }: {
  name: string;
  busy: boolean;
  online?: boolean;
  onSignOut?: () => Promise<boolean>;
  onRemove?: (alsoSignOut: boolean) => Promise<boolean>;
  renderRow?: (menu: ReactNode) => ReactNode;
  identityPeer?: string | null;
  rename?: MachineRename;
  sameNameOthers?: number;
}) {
  const [confirm, setConfirm] = useState<"sign_out" | "remove" | null>(null);
  const [alsoSignOut, setAlsoSignOut] = useState(false);
  const [acting, setActing] = useState(false);
  const [editingIdentity, setEditingIdentity] = useState<"appearance" | "name" | null>(null);
  const anchor = useRef<HTMLButtonElement | null>(null);
  const returnFocus = useRef(false);
  const identity = useMeshMachineIdentity(identityPeer, identityPeer !== undefined);
  const disabled = busy || acting;
  useLayoutEffect(() => {
    if (editingIdentity || !returnFocus.current) return;
    returnFocus.current = false;
    anchor.current?.focus({ preventScroll: true });
  }, [editingIdentity]);
  const closeIdentity = () => {
    returnFocus.current = true;
    setEditingIdentity(null);
  };
  const submit = async () => {
    setActing(true);
    try {
      const succeeded = confirm === "remove" && onRemove ? await onRemove(online || alsoSignOut) : await onSignOut?.();
      if (succeeded) { setConfirm(null); setAlsoSignOut(false); }
    } finally {
      setActing(false);
    }
  };
  if (confirm) return <div role="group" aria-label={`${confirm === "remove" ? "Remove" : "Sign out"} ${name}`} className="flex flex-col gap-2 px-3.5 py-2.5">
    <p className={UI_TEXT.meta}>{confirm === "remove"
      ? (online ? `Remove ${name} from mesh? It will be signed out first.` : `Remove ${name} from mesh?`)
      : `Sign out ${name}? It will need to sign in again to use your account.`}</p>
    {confirm === "remove" && !online && onSignOut && <label className={UI_TEXT.meta}>
      <input type="checkbox" checked={alsoSignOut} disabled={disabled} onChange={(event) => setAlsoSignOut(event.target.checked)} /> Also sign it out
    </label>}
    <div className="flex gap-2">
      <button type="button" disabled={disabled} aria-busy={disabled} className={`${UI_BUTTON.danger} ${UI_BUTTON.compact}`} onClick={() => void submit()}>
        {confirm === "remove" ? "Confirm remove" : "Confirm sign out"}
      </button>
      <button type="button" disabled={disabled} className={`${UI_BUTTON.ghost} ${UI_BUTTON.compact}`} onClick={() => { setConfirm(null); setAlsoSignOut(false); }}>Cancel</button>
    </div>
  </div>;
  const menu = onSignOut || onRemove || identityPeer !== undefined || sameNameOthers ? <span className="inline-flex">
    <DropdownMenu placement="bottom-end" disabled={disabled}
    trigger={<button ref={anchor} type="button" disabled={disabled} aria-label={`More for ${name}`} className="flex h-[26px] w-[26px] shrink-0 items-center justify-center rounded text-[18px] text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]">⋯</button>}
    items={[
      ...(identityPeer !== undefined ? [{ label: "Change icon and colour", onClick: () => setEditingIdentity("appearance") }] : []),
      ...(sameNameOthers ? [{ label: "Rename…", description: sameMachineNameDescription(sameNameOthers),
        disabled: !rename, onClick: () => setEditingIdentity("name") }] : []),
      ...(onSignOut ? [{ label: "Sign out this machine", onClick: () => setConfirm("sign_out") }] : []),
      ...(onRemove ? [{ label: "Remove from mesh", danger: true, onClick: () => setConfirm("remove") }] : []),
    ]} />
    {editingIdentity && <MachineIdentityPopover name={name} peerId={identityPeer ?? undefined}
      rename={rename} focusName={editingIdentity === "name"} initial={identity ?? UNKNOWN_MACHINE} onClose={closeIdentity} anchor={anchor} />}
  </span> : null;
  return renderRow ? renderRow(menu) : menu;
}
