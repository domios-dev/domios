import { useCallback,useEffect,useRef,useState,type ComponentProps } from "react";
import type { Tab } from "../../lib/types";
import { useAgentDescriptor } from "../../lib/useAgentDescriptor";
import { useProviderAccounts } from "../../hooks/useProviderAccounts";
import { useAccountTransition } from "../../hooks/useAccountTransition";
import { selectedDaemonTransport } from "../../lib/daemonRuntime";
import { RuntimeLoginActions } from "./RuntimeLoginActions";
import { BusyState } from "../shared/BusyState";
import { InlineErrorNotice } from "../shared/ErrorNotice";

type Props=ComponentProps<typeof RuntimeLoginActions> & {tab?:Tab|null};
const actionClass="ct-bar-pill";

export function AccountRuntimeLoginActions({tab,...props}:Props) {
  const descriptor=useAgentDescriptor(tab?.agent_type);
  return descriptor?.accounts && tab && (tab.account?.id ?? "default")!=="default" && !tab.remote_peer_id
    ? <NamedAccountLogin key={`${tab.id}:${tab.session_binding_nonce}`} tab={tab} {...props}/>
    : <RuntimeLoginActions {...props}/>;
}

function NamedAccountLogin({tab,...props}:Props & {tab:Tab}) {
  const provider=tab.agent_type!;
  const account=tab.account!.id!;
  const accounts=useProviderAccounts(provider);
  const transition=useAccountTransition(tab.id,provider);
  const row=accounts.snapshot?.accounts.find(row=>row.id===account);
  const [failure,setFailure]=useState<string|null>(null);
  const [starting,setStarting]=useState(false);
  const attempted=useRef(false);
  const active=useRef(true);
  const inFlight=useRef(false);
  useEffect(()=>{active.current=true;return()=>{active.current=false;};},[]);
  const changing=transition.transition?.status==="waiting" || transition.transition?.status==="switching";
  const start=useCallback(async()=>{
    if(inFlight.current || row?.status==="removing")return;
    inFlight.current=true;
    setStarting(true);setFailure(null);
    try{await accounts.onAction({action:"signIn",accountId:account});}
    catch(error){if(active.current)setFailure(String(error));}
    finally{inFlight.current=false;if(active.current)setStarting(false);}
  },[accounts.onAction,account,row?.status]);
  useEffect(()=>{
    if(attempted.current || !accounts.snapshot || accounts.snapshot.signInId || transition.loading || transition.error || changing || accounts.error || row?.status==="removing")return;
    attempted.current=true;
    void start();
  },[accounts.snapshot,accounts.error,transition.loading,transition.error,changing,start,row?.status]);
  if(row?.login) return <><RuntimeLoginActions {...props} providerId={provider} tabId={row.login.tabId} state={row.login.state}
    title={`Sign in to ${props.state.loginProviderName ?? props.state.agentName ?? "the harness"} as ${row.name ?? row.identity ?? "this account"}`}
    hint={row.login.state.notice ?? "Finish signing in to update this account."}
    onCancel={()=>{void accounts.onAction({action:"cancelSignIn",accountId:account}).catch(error=>setFailure(String(error)));}} onCheck={accounts.onRefresh}/>{failure && <InlineErrorNotice error={failure} operation="updating account sign-in"/>}</>;
  const refreshSaved=async()=>{
    if(inFlight.current)return;
    inFlight.current=true;
    setStarting(true);setFailure(null);
    try{await selectedDaemonTransport.json("POST",`/api/tabs/${encodeURIComponent(tab.id)}/account/refresh`,{nonce:tab.session_binding_nonce,epoch:props.state.epoch});transition.onRefresh();}
    catch(error){if(active.current)setFailure(String(error));}
    finally{inFlight.current=false;if(active.current)setStarting(false);}
  };
  const busy=starting || changing || accounts.loading || transition.loading;
  const reason=failure ?? accounts.error ?? transition.error ?? (transition.transition?.status==="failed"?transition.transition.reason:null);
  const state={...props.state,login:undefined,authentication:undefined,loginChoices:undefined,loginMethods:[],requiredSecrets:undefined};
  return <RuntimeLoginActions {...props} state={state} hint={changing?"Updating this account's sign-in.":accounts.snapshot?.signInId?"Finish the current sign-in in Harness settings.":"Sign in through this account's private sign-in card."}
    onCheck={undefined} actions={<div className="flex flex-col gap-2">
      {busy?<BusyState inline label={changing?"Updating sign-in":"Checking account"}/>:<div className="flex gap-2"><button type="button" className={actionClass} onClick={()=>{void start();}}>Sign in</button><button type="button" className={actionClass} onClick={()=>{void refreshSaved();}}>Use saved sign-in</button></div>}
      {reason && <InlineErrorNotice error={reason} operation="updating account sign-in"/>}
    </div>}/>;
}
