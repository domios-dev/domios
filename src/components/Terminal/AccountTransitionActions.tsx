import { useState } from "react";
import type { Tab } from "../../lib/types";
import { useAgentDescriptor } from "../../lib/useAgentDescriptor";
import { useTabAccounts } from "../../hooks/useAccountSelectors";
import { useMultiplexer, useMuxState } from "../../store/multiplexer/hooks";
import { RuntimeLoginActions } from "./RuntimeLoginActions";
import { BusyState } from "../shared/BusyState";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { managedTabPresentation } from "../../lib/managedTab";

export function AccountTransitionActions({tab}:{tab:Tab}) {
  const descriptor=useAgentDescriptor(tab.agent_type);
  return descriptor?.accounts && !tab.remote_peer_id
    ? <AccountTransition key={`${tab.id}:${tab.session_binding_nonce}`} tab={tab} displayName={descriptor.display_name}/>:null;
}

function AccountTransition({tab,displayName}:{tab:Tab;displayName:string}) {
  const accounts=useTabAccounts(tab);
  const {spawnAgentTab,selectTabByTab}=useMultiplexer();
  const state=useMuxState();
  const [busy,setBusy]=useState(false);
  const [error,setError]=useState<string|null>(null);
  const transition=accounts.transition;
  if(!transition || transition.status==="continued")return null;
  if(transition.status==="waiting" || transition.status==="switching")return <div role="status"><BusyState inline label={transition.cause==="reauth"?"Updating sign-in":"Switching account"}/></div>;
  const all=transition.status==="allExhausted";
  const verification=transition.cause==="verification";
  if (transition.status === "failed" && transition.continuation !== "unsupported") {
    const target = transition.nextIdentity ?? "this account";
    const reason = transition.reason ?? "";
    return <InlineErrorNotice description={{
      title: verification ? `Couldn't try ${target}.` : `Couldn't switch to ${target}.`,
      cause: transition.sourceAccountRestored ? `The conversation is back on ${transition.accountIdentity}.` : reason,
      actions: [],
      raw: reason,
    }} />;
  }
  const limit=transition.cause==="limit";
  const supported=transition.continuation==="proven" && transition.turnId!=null;
  const next=transition.nextAccountId;
  const nextIdentity=transition.nextIdentity;
  const reset=transition.resetsAtMs==null?null:new Date(transition.resetsAtMs).toLocaleTimeString([], {hour:"2-digit",minute:"2-digit"});
  const title=verification && transition.continuation==="unsupported"?`${displayName} can't move this conversation to another account`:all?`All ${displayName} accounts reached their limits`:limit?`${transition.accountIdentity} reached its limit`:"Couldn't switch accounts";
  const hint=verification && transition.continuation==="unsupported"?transition.sourceAccountRestored?`The conversation is back on ${transition.accountIdentity}.`:"The original account could not be restored. Review Details before continuing.":all?`${reset?`The first one resets at ${reset}. `:""}Send again after that.`:limit && !supported
    ?transition.continuation==="unsupported"?`${displayName} can't move this conversation to another account.`:"Moving this conversation to another account isn't confirmed yet."
    :reset?`It resets at ${reset}.`:transition.reason ?? "Choose an available account.";
  const locked=managedTabPresentation(tab.managed).locked;
  const act=async()=>{
    if(busy || locked || !next)return;
    setBusy(true);setError(null);
    try {
      if(supported && transition.turnId)await accounts.continueOn(next,transition.turnId);
      else {
        const workspace=state.workspaces.find(workspace=>workspace.tabs.includes(tab.id));
        if(!workspace || !tab.agent_type)throw new Error("The tab workspace is unavailable");
        const created=await spawnAgentTab(workspace.id,tab.agent_type,tab.cwd ?? undefined,undefined,undefined,undefined,undefined,undefined,undefined,next);
        if(created)await selectTabByTab(created.workspaceId,created.tabId);
      }
    } catch(failure){setError(String(failure));}finally{setBusy(false);}
  };
  return <RuntimeLoginActions tabId={tab.id} providerId={tab.agent_type ?? undefined} state={{state:"loaded",epoch:tab.agent_runtime_state?.epoch ?? 0}} title={title} hint={hint}
    actions={<div role="status" className="flex flex-col gap-2">
      {(all || !next)?<button type="button" className="ct-bar-pill" onClick={()=>window.dispatchEvent(new CustomEvent("codeterm:open-panel",{detail:{panel:`settings:accounts:${tab.agent_type}`}}))}>Add account</button>
        :next && <button type="button" className="ct-bar-pill" disabled={busy || locked} onClick={()=>{void act();}}>{supported?`Continue on ${nextIdentity}`:`Start a new tab on ${nextIdentity}`}</button>}
      {(error ?? accounts.error) && <InlineErrorNotice error={error ?? accounts.error!} operation="switching accounts"/>}
      {(transition.reason || all) && <details><summary>Details</summary>{transition.reason}{all && accounts.snapshot?.accounts.map(account=><div key={account.id}>{account.name ?? account.identity ?? displayName}{account.resetsAtMs?` — ${new Date(account.resetsAtMs).toLocaleTimeString()}`:""}</div>)}</details>}
    </div>}/>;
}
