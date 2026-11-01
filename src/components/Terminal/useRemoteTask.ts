import {useEffect, useRef, useState} from "react";
import {DEV_BUILD} from "../../lib/devBuild";
import {invoke} from "../../lib/bridge";
import type {RemoteTaskBinding} from "../../../packages/plugin-sdk/src/remoteTasks";

export type RemoteTaskCall = (method: "read" | "refresh" | "submit", args?: {prompt: string; branch: string}) => Promise<RemoteTaskBinding | null>;

export function remoteTaskCall(tabId: string, pluginId: string, sessionId: string): RemoteTaskCall {
  return (method, input) => invoke("plugin_remote_task_call", {tabId, pluginId, sessionId, method, args: input});
}

export function useRemoteTask(call: RemoteTaskCall) {
  const [binding, setBinding] = useState<RemoteTaskBinding | null>(null);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState<unknown>(null);
  const [readUnavailable, setReadUnavailable] = useState(false);
  const bindingRef = useRef(binding);
  bindingRef.current = binding;
  useEffect(() => {
    let active = true;
    setBusy(true);
    void call("read").then(value => {if (active) {bindingRef.current = value; setBinding(value); setReadUnavailable(false); setError(null);}})
      .catch(error => {
        if (!active) return;
        if (DEV_BUILD) console.debug("Background remote task read failed", error);
        if (!bindingRef.current) setReadUnavailable(true);
      })
      .finally(() => {if (active) setBusy(false);});
    return () => {active = false;};
  }, [call]);
  const run = async (method: "refresh" | "submit", input?: {prompt: string; branch: string}) => {
    setBusy(true); setError(null);
    try {
      const next = await call(method, input);
      bindingRef.current = next;
      setBinding(next);
      setReadUnavailable(false);
    } catch (error) {setError(error);} finally {setBusy(false);}
  };
  return {binding, busy, error, readUnavailable, run};
}
