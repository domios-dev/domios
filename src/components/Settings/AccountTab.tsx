import { UserRound } from "lucide-react";
import { SettingsCard, SettingsPage } from "./shared";
import { useAccount } from "./useAccount";
export function AccountTab() {
  const { model, signIn, signOut } = useAccount();
  return <SettingsPage title="Account">
    <SettingsCard icon={<UserRound size={16} />} title="Domios account" description="Your identity on this machine">
      {model.kind === "signed_in" && <div>
        <p>{model.identity.display_name ?? model.identity.email ?? model.identity.uid}</p>
        {model.identity.email && <p>{model.identity.email}</p>}
        <p>UID: {model.identity.uid}</p>
        <button type="button" onClick={() => void signOut()}>Sign out</button>
      </div>}
      {model.kind === "pending" && <p role="status">Signing in or loading account…</p>}
      {model.kind === "error" && <p role="alert">{model.message}</p>}
      {(model.kind === "signed_out" || model.kind === "error") && <button type="button" onClick={() => void signIn()}>Sign in</button>}
    </SettingsCard>
  </SettingsPage>;
}
