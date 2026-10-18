import {useEffect, useState} from "react";
import {invoke} from "../../lib/bridge";
import type {RemoteTaskBinding} from "../../../packages/plugin-sdk/src/remoteTasks";

export type RemoteTaskCall = (method: "read" | "refresh" | "submit", args?: {prompt: string; branch: string}) => Promise<RemoteTaskBinding | null>;

export function remoteTaskCall(pluginId: string, sessionId: string): RemoteTaskCall {
  return (method, input) => invoke("plugin_view_call", {pluginId, method, args: {sessionId, ...input}});
}

export function useRemoteTask(call: RemoteTaskCall) {
  const [binding, setBinding] = useState<RemoteTaskBinding | null>(null);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState<unknown>(null);
  useEffect(() => {
    let active = true;
    setBusy(true);
    void call("read").then(value => {if (active) {setBinding(value); setError(null);}})
      .catch(error => {if (active) setError(error);})
      .finally(() => {if (active) setBusy(false);});
    return () => {active = false;};
  }, [call]);
  const run = async (method: "refresh" | "submit", input?: {prompt: string; branch: string}) => {
    setBusy(true); setError(null);
    try {setBinding(await call(method, input));} catch (error) {setError(error);} finally {setBusy(false);}
  };
  return {binding, busy, error, run};
}
