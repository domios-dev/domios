import {useState} from "react";
import {useMuxSelector} from "../../store/multiplexer/hooks";
import type {AgentRuntimeState} from "../../lib/agentRuntimeState";
import {rowControls, selectRuntimeControl} from "../../lib/runtimeDeclaredControls";
import {InlineErrorNotice} from "../shared/ErrorNotice";
import {UI_INPUT} from "../shared/design";

/** Whether a runtime state has anything for the controls row: a notice, a mode, or a control the model picker does not own. */
export function hasRowControls(state:AgentRuntimeState|null|undefined):boolean {
  return Boolean(state?.notice || rowControls(state?.controls).length || state?.controls?.modes?.length);
}

/** The agent's declared controls other than model and reasoning effort, which the status-line model picker shows. */
export function RuntimeDeclaredControls({tabId,state}:{tabId:string;state:AgentRuntimeState}) {
  const nonce=useMuxSelector(mux=>mux.tabs[tabId]?.session_binding_nonce);
  const thread=useMuxSelector(mux=>mux.tabs[tabId]?.agent_session_id);
  const [busy,setBusy]=useState(false);
  const [error,setError]=useState<string|null>(null);
  const [dismissed,setDismissed]=useState<string|null>(null);
  const options=rowControls(state.controls);
  async function select(id:string,value:string,mode=false) {
    setBusy(true);setError(null);
    try {await selectRuntimeControl(tabId,{threadId:thread,bindingNonce:nonce,epoch:state.epoch},id,value,mode);}
    catch {setError("The agent refused this control value.");}
    finally {setBusy(false);}
  }
  return <div>
    {state.notice && dismissed !== state.notice && <div role="status">{state.notice}<button type="button" onClick={()=>setDismissed(state.notice!)}>Dismiss notice</button></div>}
    {(options.length > 0 || Boolean(state.controls?.modes?.length)) && <div role="group" aria-label="Agent controls" className="ct-status-identity-wrap flex-wrap">
    {options.map(option=><label key={option.id} className="ct-status-identity"><span className="ct-model-picker__label">{option.name}</span>
      {option.options.length ? <select role="combobox" className={UI_INPUT.select} aria-label={option.name} value={option.currentValue} disabled={busy} onChange={event=>void select(option.id,event.target.value)}>
        {!option.options.some(value=>value.value === option.currentValue) && <option value={option.currentValue}>{option.currentValue}</option>}
        {option.options.map(value=><option key={value.value} value={value.value}>{value.name}</option>)}
      </select> : <output>{option.currentValue}</output>}
    </label>)}
    {Boolean(state.controls?.modes?.length) && <label className="ct-status-identity"><span className="ct-model-picker__label">Session mode</span><select role="combobox" className={UI_INPUT.select} aria-label="Session mode" value={state.controls?.mode ?? ""} disabled={busy} onChange={event=>void select("",event.target.value,true)}>
      {!state.controls?.modes?.some(mode=>mode.id === state.controls?.mode) && <option value={state.controls?.mode ?? ""}>{state.controls?.mode ?? "Unknown"}</option>}
      {state.controls?.modes?.map(mode=><option key={mode.id} value={mode.id}>{mode.name}</option>)}
    </select></label>}
    </div>}
    {error && <InlineErrorNotice error={error} operation="changing agent controls" />}
  </div>;
}
