import { useEffect, useRef, useState } from "react";
import { DEV_BUILD } from "../../lib/devBuild";
import { localFetch } from "../../lib/webApi";
import { listen } from "../../lib/bridge";
import { onResync } from "../../lib/resync";
import { applyCloudAvailability } from "../../lib/cloudMachines/availability";
import { applyAccountPurchaseSwitch } from "../billing/purchaseSwitch";
import { accountFailure, accountModel, accountLoginPath, type LoginHint, type AccountModel, type AccountStatus, type SignInFailure, type SignInStage } from "./accountModel";

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

type LoginEvent = { attempt_id: string; stage?: SignInStage; settled?: boolean; failure?: SignInFailure | null };
type LoginWaiter = { id: string; resolve: (failure: SignInFailure | null) => void };

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await localFetch(path, init);
  if (!response.ok) throw new Error("Account request failed");
  return response.json() as Promise<T>;
}

export function useAccount(onSignOutFailure?: (message: string) => void) {
  const [model, setModel] = useState<AccountModel>({ kind: "pending" });
  const [attempt, setAttempt] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const active = useRef(0);
  const acting = useRef(false);
  const mounted = useRef(false);
  const waiter = useRef<LoginWaiter | null>(null);

  function settle(id: string, failure?: SignInFailure | null) {
    if (waiter.current?.id !== id) return;
    const { resolve } = waiter.current;
    waiter.current = null;
    resolve(failure?.error === "cancelled" ? null : failure ?? null);
  }

  function applyStatus(status: AccountStatus) {
    applyAccountPurchaseSwitch(status);
    setModel(accountModel(status));
    setAttempt(status.pending ? status.login_attempt_id ?? null : null);
    const waiting = waiter.current?.id;
    if (waiting && waiting !== status.login_attempt_id) settle(waiting);
    if (!status.pending && status.login_attempt_id) settle(status.login_attempt_id, status.last_sign_in_failure);
  }

  useEffect(() => {
    mounted.current = true;
    const controller = new AbortController();
    const refresh = () => {
      const generation = ++active.current;
      request<AccountStatus>("/api/account/status", { signal: controller.signal }).then((status) => {
        if (!controller.signal.aborted && generation === active.current) {
          applyCloudAvailability(status.cloud_machines_enabled);
          applyStatus(status);
        }
      }).catch((error: unknown) => {
        if (controller.signal.aborted || generation !== active.current) return;
        applyCloudAvailability(false);
        if (DEV_BUILD) console.debug("Background account status refresh failed", error);
        setModel((current) => current.kind === "pending" ? { kind: "signed_out" } : current);
      });
    };
    const unlisteners = [
      listen("account-changed", refresh),
      listen<LoginEvent>("account-sign-in", (event) => {
        const login = event.payload;
        if (login.settled) settle(login.attempt_id, login.failure);
        if (login.settled || login.stage === "preparation") refresh();
      }),
    ];
    for (const registration of unlisteners) void registration.catch(() => undefined);
    refresh();
    const stopResync = onResync(refresh);
    window.addEventListener("focus", refresh);
    window.addEventListener("domios:account-changed", refresh);
    window.addEventListener("domios:account-sign-in", refresh);
    return () => {
      mounted.current = false;
      controller.abort();
      for (const registration of unlisteners) void registration.then(stop => stop()).catch(() => undefined);
      waiter.current?.resolve(null);
      waiter.current = null;
      stopResync();
      window.removeEventListener("focus", refresh);
      window.removeEventListener("domios:account-changed", refresh);
      window.removeEventListener("domios:account-sign-in", refresh);
    };
  }, []);

  async function signIn(hint?: LoginHint): Promise<SignInFailure | null> {
    if (acting.current || attempt) return null;
    acting.current = true;
    setStarting(true);
    try {
      const { attempt_id } = await request<{ attempt_id: string }>(accountLoginPath(hint), { method: "POST" });
      if (!mounted.current) return null;
      const result = new Promise<SignInFailure | null>((resolve) => { waiter.current = { id: attempt_id, resolve }; });
      active.current++;
      setAttempt(attempt_id);
      window.dispatchEvent(new Event("domios:account-sign-in"));
      return await result;
    } catch {
      try {
        const status = await request<AccountStatus>("/api/account/status");
        if (status.pending && mounted.current) {
          applyStatus(status);
          return null;
        }
      } catch {
        // The original request failed too.
      }
      const failure: SignInFailure = { stage: "preparation", error: "transport" };
      if (mounted.current) setModel(current => current.kind === "signed_in" ? current
        : { kind: "error", message: signInFailureToastMessage(failure), failure });
      return failure;
    } finally {
      if (mounted.current) setStarting(false);
      acting.current = false;
    }
  }

  async function cancelSignIn() {
    if (!attempt) return;
    try {
      await localFetch(`/api/account/login/${encodeURIComponent(attempt)}/cancel`, { method: "POST" });
    } catch (error) {
      if (DEV_BUILD) console.debug("Sign-in cancellation request failed", error);
    } finally {
      window.dispatchEvent(new Event("domios:account-sign-in"));
    }
  }

  async function signOut() {
    try {
      const status = await request<AccountStatus>("/api/account/logout", { method: "POST" });
      active.current++;
      applyStatus(status);
      if (status.server_stop_unconfirmed) onSignOutFailure?.("Signed out locally. Server stop unconfirmed; Domios will retry when connected.");
      window.dispatchEvent(new Event("domios:account-changed"));
    } catch (error) {
      setModel(accountFailure(error));
      onSignOutFailure?.("Sign-out could not be completed. Try again.");
    }
  }

  return { model, signIn, signOut, signingIn: starting || attempt !== null, cancelSignIn: attempt ? cancelSignIn : undefined };
}
