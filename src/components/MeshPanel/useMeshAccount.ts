import { useCallback } from "react";
import { createNoticePublisher } from "../../lib/notices";
import { useAccount } from "../Settings/useAccount";
import { signInFailureToast } from "../Settings/accountFailureToast";
import { useMultiplexer } from "../../store/multiplexer/context";
import { accountPresence, type AccountPresence } from "./accountMachines";

/** The signed-in state Mesh shows and the Sign in action, with its failure toast. */
export function useMeshAccount(): { presence: AccountPresence; signIn: () => Promise<void> } {
  const account = useAccount();
  const { upsertToast } = useMultiplexer();
  const signIn = useCallback(async () => {
    const failure = await account.signIn();
    if (failure) createNoticePublisher({ upsertToast })(signInFailureToast(failure, () => { void account.signIn(); }));
  }, [account, upsertToast]);
  return { presence: accountPresence(account.model.kind), signIn };
}
