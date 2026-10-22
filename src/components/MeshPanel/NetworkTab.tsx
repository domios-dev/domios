import { useCallback } from "react";
import { createNoticePublisher } from "../../lib/notices";
import { Users } from "lucide-react";
import MyMachines from "./AddPeerPanel";
import { accountPresence, meshFailureToast } from "./accountMachines";
import type { AccountMeshFailure } from "./accountMeshClient";
import { SettingsPage, SettingsCard } from "../Settings/shared";
import { useAccount } from "../Settings/useAccount";
import { signInFailureToast } from "../Settings/accountFailureToast";
import { useMultiplexer } from "../../store/multiplexer/context";

interface Props {
  triggerRefresh: () => void;
}

export function NetworkTab({ triggerRefresh }: Props) {
  const account = useAccount();
  const { upsertToast } = useMultiplexer();

  const signIn = useCallback(async () => {
    const failure = await account.signIn();
    if (failure) createNoticePublisher({ upsertToast })(signInFailureToast(failure, () => { void account.signIn(); }));
  }, [account, upsertToast]);

  const onFailure = useCallback((operation: string, failure: AccountMeshFailure) => {
    upsertToast(meshFailureToast(operation, failure));
  }, [upsertToast]);

  return (
    <div className="h-full overflow-y-auto">
      <div className="p-6">
        <SettingsPage title="Connect your machines">
          <SettingsCard
            icon={<Users size={16} />}
            title="My machines"
            description="Machines signed in to your Domios account. Add links one directly; both machines must be online."
          >
            <MyMachines
              account={accountPresence(account.model.kind)}
              onSignIn={signIn}
              onFailure={onFailure}
              onChanged={triggerRefresh}
            />
          </SettingsCard>
        </SettingsPage>
      </div>
    </div>
  );
}
