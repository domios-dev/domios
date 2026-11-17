import {useEffect,useRef,useState} from "react";
import {invoke} from "../../lib/bridge";
import {nativeBridge} from "../../lib/nativeBridge";
import type {AgentRuntimeState} from "../../lib/agentRuntimeState";
import {runtimeLoginErrorMessage} from "./runtimeLoginError";

export function useRuntimeLogin(tabId:string, providerId?:string, state?:AgentRuntimeState) {
  const [busy,setBusy]=useState(false);
  const [error,setError]=useState<string|null>(null);
  const pending=useRef(false);
  const cancelPending=useRef(false);
  const cycle=useRef(0);
  const current=useRef(false);
  useEffect(()=>{current.current=true;return ()=>{current.current=false;};},[]);
  async function operate(request:{action?:string;method?:string;methodId?:string;apiKey?:string;account?:string;code?:string}) {
    const cancel=request.action === "cancel";
    if (cancel ? cancelPending.current : pending.current) return false;
    if (cancel) { cancelPending.current=true;cycle.current++; } else pending.current=true;
    const ownCycle=cycle.current;
    const owns=()=>current.current && ownCycle===cycle.current;
    setBusy(true);setError(null);
    try {
      const result=await invoke<{login?:AgentRuntimeState["login"]}>("agent_runtime_login",{tabId,request:{...request,...(providerId ? {providerId} : {}),
        ...(state?.loginBinding ? {binding:state.loginBinding} : {}),
        ...(request.action && state?.login?.loginId ? {loginId:state.login.loginId} : {})}});
      if (owns() && !request.action && (request.method === "browser" || request.methodId) && result.login?.url && !tabId.startsWith("mesh:")) await nativeBridge.openUrl(result.login.url);
      return owns();
    } catch (failure) {if(owns()) {
      const secret=request.apiKey ?? request.code;
      const message=runtimeLoginErrorMessage(failure,request.action);
      setError(secret ? message.replaceAll(secret,"<redacted>") : message);
    } return false;}
    finally {if(cancel)cancelPending.current=false;else pending.current=false;if(current.current)setBusy(pending.current||cancelPending.current);}
  }
  return {busy,error,operate};
}
