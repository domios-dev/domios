import { useCallback } from "react";
import { Users } from "lucide-react";
import MyMachines from "./AddPeerPanel";
import { meshFailureToast } from "./accountMachines";
import type { AccountMeshFailure } from "./accountMeshClient";
import { SettingsPage, SettingsCard } from "../Settings/shared";
import { useMultiplexer } from "../../store/multiplexer/context";
import { useMeshAccount } from "./useMeshAccount";

interface Props {
  triggerRefresh: () => void;
}

export function NetworkTab({ triggerRefresh }: Props) {
  const account = useMeshAccount();
  const { upsertToast } = useMultiplexer();

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
            description="Machines signed in to your Domios account connect automatically when both are online."
          >
            <MyMachines
              account={account.presence}
              onSignIn={account.signIn}
              onFailure={onFailure}
              onChanged={triggerRefresh}
            />
          </SettingsCard>
        </SettingsPage>
      </div>
    </div>
  );
}
