import {useEffect,useRef,useState} from "react";
import {invoke} from "../../lib/bridge";
import {nativeBridge} from "../../lib/nativeBridge";
import type {AgentRuntimeState} from "../../lib/agentRuntimeState";

export function useRuntimeLogin(tabId:string) {
  const [busy,setBusy]=useState(false);
  const [error,setError]=useState<string|null>(null);
  const pending=useRef(false);
  const current=useRef(false);
  useEffect(()=>{current.current=true;return ()=>{current.current=false;};},[]);
  async function operate(request:{action?:string;method?:string;apiKey?:string}) {
    if (pending.current) return;
    pending.current=true;setBusy(true);setError(null);
    try {
      const result=await invoke<{login?:AgentRuntimeState["login"]}>("agent_runtime_login",{tabId,request});
      if (current.current && request.method === "browser" && result.login?.url && !tabId.startsWith("mesh:")) await nativeBridge.openUrl(result.login.url);
    } catch {if(current.current)setError("Sign-in failed. Try again.");}
    finally {pending.current=false;if(current.current)setBusy(false);}
  }
  return {busy,error,operate};
}
