import { useRef, useState } from "react";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { runtimeLoginErrorMessage } from "./runtimeLoginError";

export function credentialOwnerFailure(error?: string): boolean {
  return /\bos error 1307\b|\bERROR_INVALID_OWNER\b/i.test(error ?? "");
}

export function CredentialOwnerRecovery({ loginId, error, onRecover, busy = false }: {
  tabId: string;
  providerId?: string;
  loginId?: string;
  error?: string;
  onRecover: () => Promise<void>;
  busy?: boolean;
}) {
  const pending = useRef(false);
  const [working, setWorking] = useState(false);
  const [failure, setFailure] = useState<string>();
  const [recoveredLogin, setRecoveredLogin] = useState<string>();
  if (!loginId || !credentialOwnerFailure(error)) return null;
  if (recoveredLogin === loginId) return <p role="status">The saved sign-in is backed up. Choose a sign-in method to try again.</p>;

  const recover = async () => {
    if (pending.current || busy) return;
    pending.current = true;
    setWorking(true);
    setFailure(undefined);
    try {
      await onRecover();
      setRecoveredLogin(loginId);
    } catch (reason) {
      setFailure(runtimeLoginErrorMessage(reason));
    } finally {
      pending.current = false;
      setWorking(false);
    }
  };

  return <div className="flex w-full flex-col gap-2 text-[13px] text-[var(--ct-muted-fg)]">
    <p>Windows cannot replace the saved sign-in because it belongs to another owner. Move it to a dated backup in the same folder, then sign in again.</p>
    <button type="button" disabled={busy || working} onClick={() => { void recover(); }}
      className="self-start rounded-full border border-[var(--ct-border)] px-3.5 py-1.5 text-[var(--ct-fg)] disabled:opacity-50">
      {working ? "Moving saved sign-in…" : "Move saved sign-in to backup"}
    </button>
    {failure && <InlineErrorNotice error={failure} operation="backing up saved sign-in" blocking />}
  </div>;
}
