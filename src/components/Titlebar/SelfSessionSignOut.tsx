import { useId, useState } from "react";
import { FormDialog } from "../shared/FormDialog";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { UI_BUTTON, UI_TEXT } from "../shared/design";
import { useAccountMeshClient } from "../MeshPanel/accountMeshClient";
import { failureMessage } from "../MeshPanel/accountMachines";

export function SelfSessionSignOut({ onClose }: { onClose: () => void }) {
  const client = useAccountMeshClient();
  const title = useId();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [signedOut, setSignedOut] = useState(false);
  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      const result = await client.revokeSession();
      if (result.ok) { onClose(); return; }
      if (result.local_signed_out) { setSignedOut(true); return; }
      setError(failureMessage(result.failure));
    } catch {
      setError("This machine could not be signed out. Try again.");
    } finally {
      setBusy(false);
    }
  };
  const close = () => { if (!busy) onClose(); };
  return <FormDialog titleId={title} onCancel={close} onSubmit={(event) => { event.preventDefault(); if (signedOut) close(); else if (!busy) void submit(); }}>
    <h2 id={title}>{signedOut ? "Signed out" : "Sign out this machine?"}</h2>
    <p className={UI_TEXT.meta} role={signedOut ? "status" : undefined}>{signedOut
      ? "You are signed out. This machine is still listed on your account because the account service could not confirm sign-out. You can remove it from Account > machines on any signed-in device."
      : "This machine will need to sign in again to use your account."}</p>
    {error && <InlineErrorNotice role="alert" description={{ title: "Sign-out failed", cause: error, raw: error, actions: [] }} actions={[]} />}
    <div className="flex gap-2">
      {signedOut ? <button type="button" onClick={close} className={UI_BUTTON.ghost}>Close</button> : <>
        <button type="submit" disabled={busy} aria-busy={busy} className={UI_BUTTON.danger}>Sign out this machine</button>
        <button type="button" disabled={busy} onClick={close} className={UI_BUTTON.ghost}>Cancel</button>
      </>}
    </div>
  </FormDialog>;
}
