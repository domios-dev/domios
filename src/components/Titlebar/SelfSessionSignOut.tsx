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
  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      const result = await client.revokeSession();
      if (result.ok) { onClose(); return; }
      setError(failureMessage(result.failure));
    } catch {
      setError("This machine could not be signed out. Try again.");
    } finally {
      setBusy(false);
    }
  };
  const close = () => { if (!busy) onClose(); };
  return <FormDialog titleId={title} onCancel={close} onSubmit={(event) => { event.preventDefault(); if (!busy) void submit(); }}>
    <h2 id={title}>Sign out this machine?</h2>
    <p className={UI_TEXT.meta}>This machine will need to sign in again to use your account.</p>
    {error && <InlineErrorNotice role="alert" description={{ title: "Sign-out failed", cause: error, raw: error, actions: [] }} actions={[]} />}
    <div className="flex gap-2">
      <button type="submit" disabled={busy} aria-busy={busy} className={UI_BUTTON.danger}>Sign out this machine</button>
      <button type="button" disabled={busy} onClick={close} className={UI_BUTTON.ghost}>Cancel</button>
    </div>
  </FormDialog>;
}
