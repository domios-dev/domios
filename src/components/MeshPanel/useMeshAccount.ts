import { useCallback } from "react";
import { createNoticePublisher } from "../../lib/notices";
import { useAccount } from "../Settings/useAccount";
import { signInFailureToast } from "../Settings/accountFailureToast";
import { useMultiplexer } from "../../store/multiplexer/context";
import { accountPresence, type AccountPresence } from "./accountMachines";

export interface MeshAccount {
  presence: AccountPresence;
  uid?: string | null;
  signIn: () => Promise<void>;
  signingIn: boolean;
  cancelSignIn?: () => void;
}

/** The signed-in state Mesh shows and the Sign in action, with its failure toast. */
export function useMeshAccount(): MeshAccount {
  const account = useAccount();
  const { upsertToast } = useMultiplexer();
  const signIn = useCallback(async () => {
    const failure = await account.signIn();
    if (failure) createNoticePublisher({ upsertToast })(signInFailureToast(failure, () => { void account.signIn(); }));
  }, [account, upsertToast]);
  return {
    presence: accountPresence(account.model.kind),
    uid: account.model.kind === "signed_in" ? account.model.identity.uid : null,
    signIn,
    signingIn: account.signingIn,
    cancelSignIn: account.cancelSignIn,
  };
}
