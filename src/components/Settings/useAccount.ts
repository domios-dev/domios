import { useEffect, useRef, useState } from "react";
import { DEV_BUILD } from "../../lib/devBuild";
import { localFetch } from "../../lib/webApi";
import { listen } from "../../lib/bridge";
import { onResync } from "../../lib/resync";
import { accountFailure, accountModel, accountLoginPath, type LoginHint, type AccountModel, type AccountStatus, type SignInFailure } from "./accountModel";

export function signInFailureToastMessage(failure: SignInFailure): string {
  if (failure.error === "transport") {
    return "Sign-in could not be completed: Domios could not connect to complete sign-in. Check your connection and try again.";
  }
  const stageMessage: Record<SignInFailure["stage"], string> = {
    preparation: "sign-in setup failed",
    browser: "opening the sign-in page failed",
    callback: "the callback could not be validated",
    redemption: "the code exchange failed",
    credential_save: "saving sign-in credentials failed",
    publication: "publishing the signed-in session failed",
  };
  return `Sign-in could not be completed: ${stageMessage[failure.stage]}. Choose Retry to try again.`;
}

export function useAccount() {
  const [model, setModel] = useState<AccountModel>({ kind: "pending" });
  const active = useRef(0);
  const acting = useRef(false);
  async function request(action: "status" | "login" | "logout", signal?: AbortSignal, hint?: LoginHint): Promise<AccountStatus> {
    const response = await localFetch(action === "login" ? accountLoginPath(hint) : `/api/account/${action}`, { method: action === "status" ? "GET" : "POST", signal });
    if (!response.ok) throw new Error("Account request failed");
    return response.json() as Promise<AccountStatus>;
  }
  useEffect(() => {
    const controller = new AbortController();
    const refresh = () => {
      if (acting.current) return;
      const generation = ++active.current;
      request("status", controller.signal).then((status) => {
        if (generation === active.current) setModel(accountModel(status));
      }).catch((error: unknown) => {
        if (controller.signal.aborted || generation !== active.current) return;
        if (DEV_BUILD) console.debug("Background account status refresh failed", error);
        setModel((current) => current.kind === "pending" ? { kind: "signed_out" } : current);
      });
    };
    refresh();
    let unlisten: (() => void) | undefined;
    void listen("account-changed", refresh).then((stop) => {
      if (controller.signal.aborted) stop();
      else unlisten = stop;
    }).catch(() => undefined);
    const stopResync = onResync(refresh);
    window.addEventListener("focus", refresh);
    window.addEventListener("domios:account-changed", refresh);
    return () => {
      controller.abort(); active.current++;
      unlisten?.();
      stopResync();
      window.removeEventListener("focus", refresh);
      window.removeEventListener("domios:account-changed", refresh);
    };
  }, []);
  async function act(action: "login" | "logout", hint?: LoginHint): Promise<SignInFailure | null> {
    if (acting.current) return null;
    const generation = ++active.current;
    acting.current = true;
    setModel({ kind: "pending" });
    let changed = false;
    let signInFailure: SignInFailure | null = null;
    try {
      const status = await request(action, undefined, hint);
      if (generation === active.current) setModel(accountModel(status));
      if (action === "login") signInFailure = status.last_sign_in_failure ?? null;
      changed = true;
    } catch (error) {
      let failure = accountFailure(error);
      if (action === "login") {
        try {
          const status = await request("status");
          // Another surface's sign-in holds the login; wait for it to settle rather than report a failure.
          if (status.pending) {
            if (generation === active.current) setModel({ kind: "pending" });
            return null;
          }
          if (status.last_sign_in_failure) {
            failure = accountModel(status);
            signInFailure = status.last_sign_in_failure;
            changed = true;
          }
        } catch { failure = accountFailure(error); }
        signInFailure ??= { stage: "preparation", error: "transport" };
        failure = { kind: "error", message: signInFailureToastMessage(signInFailure), failure: signInFailure };
      }
      if (generation === active.current) setModel(failure);
    } finally {
      acting.current = false;
      if (changed) window.dispatchEvent(new Event("domios:account-changed"));
    }
    return signInFailure;
  }
  return { model, signIn: (hint?: LoginHint) => act("login", hint), signOut: () => act("logout") };
}
