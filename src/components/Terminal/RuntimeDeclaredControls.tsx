import {useState} from "react";
import {useMuxSelector} from "../../store/multiplexer/hooks";
import type {AgentRuntimeState} from "../../lib/agentRuntimeState";
import {rowControls, selectRuntimeControl} from "../../lib/runtimeDeclaredControls";
import {InlineErrorNotice} from "../shared/ErrorNotice";
import {DropdownSelect} from "../shared/DropdownMenu";

/** Whether a runtime state has anything for the composer: a notice the agent sent. */
export function hasRowControls(state:AgentRuntimeState|null|undefined):boolean {
  return Boolean(state?.notice);
}

/** Whether the agent declared controls other than model and effort (a session mode and the like). */
export function hasControlPills(state:AgentRuntimeState|null|undefined):boolean {
  return Boolean(rowControls(state?.controls).length || state?.controls?.modes?.length);
}

const PILL = "!w-auto max-w-[14rem] !py-1 [font-family:var(--ct-font-ui)]";

/**
 * The agent's declared controls other than model and reasoning effort (which the status-line model picker shows).
 * `controls` are compact pills that sit beside the model bubble; `notice` is the agent's dismissible notice above the
 * composer.
 */
export function RuntimeDeclaredControls({tabId,state,parts="all"}:{tabId:string;state:AgentRuntimeState;parts?:"all"|"notice"|"controls"}) {
  const nonce=useMuxSelector(mux=>mux.tabs[tabId]?.session_binding_nonce);
  const thread=useMuxSelector(mux=>mux.tabs[tabId]?.agent_session_id);
  const [busy,setBusy]=useState(false);
  const [error,setError]=useState<string|null>(null);
  const [dismissed,setDismissed]=useState<string|null>(null);
  const options=rowControls(state.controls);
  const modes=state.controls?.modes ?? [];
  async function select(id:string,value:string,mode=false) {
    setBusy(true);setError(null);
    try {await selectRuntimeControl(tabId,{threadId:thread,bindingNonce:nonce,epoch:state.epoch},id,value,mode);}
    catch {setError("The agent refused this control value.");}
    finally {setBusy(false);}
  }
  const showNotice=parts !== "controls" && state.notice && dismissed !== state.notice;
  const showControls=parts !== "notice" && (options.length > 0 || modes.length > 0);
  return <>
    {showNotice && <div role="status" className="mx-auto mb-2 flex w-full max-w-4xl items-center gap-2 text-[12px] text-[var(--ct-muted-fg)]">
      <span className="min-w-0 flex-1">{state.notice}</span>
      <button type="button" className="shrink-0 rounded px-1.5 py-0.5 hover:text-[var(--ct-fg)]" onClick={()=>setDismissed(state.notice!)}>Dismiss notice</button>
    </div>}
    {showControls && <span role="group" aria-label="Agent controls" className="inline-flex min-w-0 flex-wrap items-center gap-1.5">
      {options.map(option=>option.options.length
        ? <DropdownSelect key={option.id} aria-label={option.name} prefix={option.name} className={PILL} disabled={busy} value={option.currentValue}
          onChange={value=>void select(option.id,value)}
          options={[...(option.options.some(value=>value.value === option.currentValue) ? [] : [{value:option.currentValue,label:option.currentValue}]),
            ...option.options.map(value=>({value:value.value,label:value.name}))]} />
        : <output key={option.id} aria-label={option.name} className="text-[12px] text-[var(--ct-muted-fg)]">{option.currentValue}</output>)}
      {modes.length > 0 && <DropdownSelect aria-label="Session mode" prefix="Mode" className={PILL} disabled={busy} value={state.controls?.mode ?? ""}
        onChange={value=>void select("",value,true)}
        options={[...(modes.some(mode=>mode.id === state.controls?.mode) ? [] : [{value:state.controls?.mode ?? "",label:state.controls?.mode ?? "Unknown"}]),
          ...modes.map(mode=>({value:mode.id,label:mode.name}))]} />}
    </span>}
    {error && <InlineErrorNotice error={error} operation="changing agent controls" />}
  </>;
}
