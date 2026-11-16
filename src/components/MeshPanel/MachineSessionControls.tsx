import { useState, type ReactNode } from "react";
import { UI_BUTTON, UI_TEXT } from "../shared/design";
import { DropdownMenu } from "../shared/DropdownMenu";

export function MachineSessionControls({ name, busy, onSignOut, onRemove, renderRow }: {
  name: string;
  busy: boolean;
  onSignOut?: () => Promise<boolean>;
  onRemove?: (alsoSignOut: boolean) => Promise<boolean>;
  renderRow?: (menu: ReactNode) => ReactNode;
}) {
  const [confirm, setConfirm] = useState<"sign_out" | "remove" | null>(null);
  const [alsoSignOut, setAlsoSignOut] = useState(false);
  const [acting, setActing] = useState(false);
  const disabled = busy || acting;
  const submit = async () => {
    setActing(true);
    try {
      const succeeded = confirm === "remove" && onRemove ? await onRemove(alsoSignOut) : await onSignOut?.();
      if (succeeded) { setConfirm(null); setAlsoSignOut(false); }
    } finally {
      setActing(false);
    }
  };
  if (confirm) return <div role="group" aria-label={`${confirm === "remove" ? "Remove" : "Sign out"} ${name}`} className="flex flex-col gap-2 px-3.5 py-2.5">
    <p className={UI_TEXT.meta}>{confirm === "remove" ? `Remove ${name} from mesh?` : `Sign out ${name}? It will need to sign in again to use your account.`}</p>
    {confirm === "remove" && onSignOut && <label className={UI_TEXT.meta}>
      <input type="checkbox" checked={alsoSignOut} disabled={disabled} onChange={(event) => setAlsoSignOut(event.target.checked)} /> Also sign it out
    </label>}
    <div className="flex gap-2">
      <button type="button" disabled={disabled} aria-busy={disabled} className={`${UI_BUTTON.danger} ${UI_BUTTON.compact}`} onClick={() => void submit()}>
        {confirm === "remove" ? "Confirm remove" : "Confirm sign out"}
      </button>
      <button type="button" disabled={disabled} className={`${UI_BUTTON.ghost} ${UI_BUTTON.compact}`} onClick={() => { setConfirm(null); setAlsoSignOut(false); }}>Cancel</button>
    </div>
  </div>;
  const menu = onSignOut || onRemove ? <DropdownMenu placement="bottom-end" disabled={disabled}
    trigger={<button type="button" disabled={disabled} aria-label={`More for ${name}`} className="flex h-[26px] w-[26px] shrink-0 items-center justify-center rounded text-[18px] text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]">⋯</button>}
    items={[
      ...(onSignOut ? [{ label: "Sign out this machine", onClick: () => setConfirm("sign_out") }] : []),
      ...(onRemove ? [{ label: "Remove from mesh", danger: true, onClick: () => setConfirm("remove") }] : []),
    ]} /> : null;
  return renderRow ? renderRow(menu) : menu;
}
