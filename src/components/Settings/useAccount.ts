import { useEffect, useRef, useState } from "react";
import { localFetch } from "../../lib/webApi";
import { accountFailure, accountModel, type AccountModel, type AccountStatus } from "./accountModel";

export function useAccount() {
  const [model, setModel] = useState<AccountModel>({ kind: "pending" });
  const active = useRef(0);
  async function request(action: "status" | "login" | "logout", signal?: AbortSignal): Promise<AccountStatus> {
    const response = await localFetch(`/api/account/${action}`, { method: action === "status" ? "GET" : "POST", signal });
    if (!response.ok) throw new Error(await response.text());
    return response.json() as Promise<AccountStatus>;
  }
  useEffect(() => {
    const controller = new AbortController();
    const generation = ++active.current;
    request("status", controller.signal).then((status) => {
      if (generation === active.current) setModel(accountModel(status));
    }).catch((error: unknown) => {
      if (!controller.signal.aborted && generation === active.current) setModel(accountFailure(error));
    });
    return () => { controller.abort(); active.current++; };
  }, []);
  async function act(action: "login" | "logout") {
    const generation = ++active.current;
    setModel({ kind: "pending" });
    try {
      const status = await request(action);
      if (generation === active.current) setModel(accountModel(status));
    } catch (error) {
      if (generation === active.current) setModel(accountFailure(error));
    }
  }
  return { model, signIn: () => act("login"), signOut: () => act("logout") };
}
