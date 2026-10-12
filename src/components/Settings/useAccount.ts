import { useEffect, useRef, useState } from "react";
import { localFetch } from "../../lib/webApi";
import { accountFailure, accountModel, accountLoginPath, type LoginHint, type AccountModel, type AccountStatus } from "./accountModel";

export function useAccount() {
  const [model, setModel] = useState<AccountModel>({ kind: "pending" });
  const active = useRef(0);
  const acting = useRef(false);
  async function request(action: "status" | "login" | "logout", signal?: AbortSignal, hint?: LoginHint): Promise<AccountStatus> {
    const response = await localFetch(action === "login" ? accountLoginPath(hint) : `/api/account/${action}`, { method: action === "status" ? "GET" : "POST", signal });
    if (!response.ok) throw new Error(await response.text());
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
        if (!controller.signal.aborted && generation === active.current) setModel(accountFailure(error));
      });
    };
    refresh();
    window.addEventListener("focus", refresh);
    window.addEventListener("domios:account-changed", refresh);
    return () => {
      controller.abort(); active.current++;
      window.removeEventListener("focus", refresh);
      window.removeEventListener("domios:account-changed", refresh);
    };
  }, []);
  async function act(action: "login" | "logout", hint?: LoginHint) {
    if (acting.current) return;
    const generation = ++active.current;
    acting.current = true;
    setModel({ kind: "pending" });
    try {
      const status = await request(action, undefined, hint);
      if (generation === active.current) setModel(accountModel(status));
      window.dispatchEvent(new Event("domios:account-changed"));
    } catch (error) {
      if (generation === active.current) setModel(accountFailure(error));
    } finally { acting.current = false; }
  }
  return { model, signIn: (hint?: LoginHint) => act("login", hint), signOut: () => act("logout") };
}
