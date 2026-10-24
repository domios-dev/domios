import { useEffect, useRef } from "react";
import { useAccount } from "./useAccount";

export function useSessionRevocationScreen(showSignIn: () => void) {
  const { model } = useAccount();
  const shown = useRef(false);
  const revoked = model.kind === "signed_out" && model.sessionRevoked === true;
  useEffect(() => {
    if (revoked && !shown.current) showSignIn();
    shown.current = revoked;
  }, [revoked, showSignIn]);
}
