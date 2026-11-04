import {useState} from "react";
import {invoke} from "../../lib/bridge";
import {useMuxSelector} from "../../store/multiplexer/hooks";
import type {AgentRuntimeState} from "../../lib/agentRuntimeState";
import {InlineErrorNotice} from "../shared/ErrorNotice";

export function RuntimeDeclaredControls({tabId,state}:{tabId:string;state:AgentRuntimeState}) {
  const nonce=useMuxSelector(mux=>mux.tabs[tabId]?.session_binding_nonce);
  const thread=useMuxSelector(mux=>mux.tabs[tabId]?.agent_session_id);
  const [busy,setBusy]=useState(false);
  const [error,setError]=useState<string|null>(null);
  const [dismissed,setDismissed]=useState<string|null>(null);
  async function select(id:string,value:string,mode=false) {
    setBusy(true);setError(null);
    try {await invoke("agent_runtime_controls",{tabId,request:{threadId:thread,bindingNonce:nonce,epoch:state.epoch,id,value,mode}});}
    catch {setError("The agent refused this control value.");}
    finally {setBusy(false);}
  }
  return <div>
    {state.notice && dismissed !== state.notice && <div role="status">{state.notice}<button type="button" onClick={()=>setDismissed(state.notice!)}>Dismiss notice</button></div>}
    {state.controls?.configOptions?.map(option=><label key={option.id}>{option.name}
      {option.options.length ? <select aria-label={option.name} value={option.currentValue} disabled={busy} onChange={event=>void select(option.id,event.target.value)}>
        {!option.options.some(value=>value.value === option.currentValue) && <option value={option.currentValue}>{option.currentValue}</option>}
        {option.options.map(value=><option key={value.value} value={value.value}>{value.name}</option>)}
      </select> : <output>{option.currentValue}</output>}
    </label>)}
    {Boolean(state.controls?.modes?.length) && <label>Session mode<select aria-label="Session mode" value={state.controls?.mode ?? ""} disabled={busy} onChange={event=>void select("",event.target.value,true)}>
      {!state.controls?.modes?.some(mode=>mode.id === state.controls?.mode) && <option value={state.controls?.mode ?? ""}>{state.controls?.mode ?? "Unknown"}</option>}
      {state.controls?.modes?.map(mode=><option key={mode.id} value={mode.id}>{mode.name}</option>)}
    </select></label>}
    {error && <InlineErrorNotice error={error} operation="changing agent controls" />}
  </div>;
}
