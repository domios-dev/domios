import { useState } from "react";
import { UI_BUTTON, UI_TEXT } from "../shared/design";

export function MachineSessionControls({ name, busy, onSignOut, onRemove }: {
  name: string;
  busy: boolean;
  onSignOut: () => Promise<boolean>;
  onRemove?: (alsoSignOut: boolean) => Promise<boolean>;
}) {
  const [confirm, setConfirm] = useState<"sign_out" | "remove" | null>(null);
  const [alsoSignOut, setAlsoSignOut] = useState(false);
  const [acting, setActing] = useState(false);
  const disabled = busy || acting;
  const submit = async () => {
    setActing(true);
    try {
      const succeeded = confirm === "remove" && onRemove ? await onRemove(alsoSignOut) : await onSignOut();
      if (succeeded) { setConfirm(null); setAlsoSignOut(false); }
    } finally {
      setActing(false);
    }
  };
  if (confirm) return <div role="group" aria-label={`${confirm === "remove" ? "Remove" : "Sign out"} ${name}`} className="flex flex-col gap-2">
    <p className={UI_TEXT.meta}>{confirm === "remove" ? `Remove ${name} from mesh?` : `Sign out ${name}? It will need to sign in again to use your account.`}</p>
    {confirm === "remove" && <label className={UI_TEXT.meta}>
      <input type="checkbox" checked={alsoSignOut} disabled={disabled} onChange={(event) => setAlsoSignOut(event.target.checked)} /> Also sign it out
    </label>}
    <div className="flex gap-2">
      <button type="button" disabled={disabled} aria-busy={disabled} className={`${UI_BUTTON.danger} ${UI_BUTTON.compact}`} onClick={() => void submit()}>
        {confirm === "remove" ? "Confirm remove" : "Confirm sign out"}
      </button>
      <button type="button" disabled={disabled} className={`${UI_BUTTON.ghost} ${UI_BUTTON.compact}`} onClick={() => { setConfirm(null); setAlsoSignOut(false); }}>Cancel</button>
    </div>
  </div>;
  return <div className="flex flex-wrap gap-1.5">
    <button type="button" disabled={disabled} aria-label={`Sign out ${name}`} className={`${UI_BUTTON.ghost} ${UI_BUTTON.compact}`} onClick={() => setConfirm("sign_out")}>Sign out this machine</button>
    {onRemove && <button type="button" disabled={disabled} aria-label={`Remove ${name} from mesh`} className={`${UI_BUTTON.ghost} ${UI_BUTTON.compact}`} onClick={() => setConfirm("remove")}>Remove from mesh</button>}
  </div>;
}
