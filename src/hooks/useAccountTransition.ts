import { useCallback,useEffect,useRef,useState } from "react";
import { listen,type UnlistenFn } from "../lib/bridge";
import { selectedDaemonTransport } from "../lib/daemonRuntime";
import type { AccountTransition } from "./providerAccountsTypes";

interface TabAccountStatus {transition:AccountTransition|null;canVerify:boolean;turnActive:boolean}

export function useAccountTransition(tabId:string|null,providerId:string|null,boundaryKey?:string) {
  const key=JSON.stringify([tabId,providerId,boundaryKey]);
  const [view,setView]=useState<{key:string;transition:AccountTransition|null;loading:boolean;error:string|null;canVerify:boolean;turnActive:boolean}>({key,transition:null,loading:Boolean(tabId),error:null,canVerify:false,turnActive:true});
  const refresh=useRef<()=>void>(()=>{});
  useEffect(()=>{
    let active=true;let sequence=0;let stop:UnlistenFn|undefined;
    const controller=new AbortController();
    setView({key,transition:null,loading:Boolean(tabId),error:null,canVerify:false,turnActive:true});
    if(!tabId || !providerId){refresh.current=()=>{};return;}
    const read=async()=>{
      if(!active)return;
      const request=++sequence;
      try {
        const status=await selectedDaemonTransport.json<TabAccountStatus>("GET",`/api/tabs/${encodeURIComponent(tabId)}/account`,undefined,{signal:controller.signal});
        if(active && request===sequence)setView({key,canVerify:status.canVerify===true,turnActive:status.turnActive!==false,transition:status.transition?.providerId===providerId?status.transition:null,loading:false,error:null});
      }catch(error){if(active && request===sequence && !controller.signal.aborted)setView(current=>({...current,key,loading:false,error:String(error)}));}
    };
    refresh.current=()=>{void read();};
    void listen<{tabId:string;transition:AccountTransition|null}>("provider-account-transition",({payload})=>{
      if(!active || payload.tabId!==tabId || (payload.transition && payload.transition.providerId!==providerId))return;
      sequence++;
      setView(current=>({...current,key,transition:payload.transition,loading:false,error:null,canVerify:false,turnActive:true}));
      void read();
    }).then(unlisten=>{if(!active){unlisten();return;}stop=unlisten;void read();}).catch(error=>{if(active)setView({key,transition:null,loading:false,error:String(error),canVerify:false,turnActive:true});});
    return()=>{active=false;sequence++;controller.abort();stop?.();};
  },[key,tabId,providerId]);
  const onRefresh=useCallback(()=>refresh.current(),[]);
  return {canVerify:view.key===key && view.canVerify,turnActive:view.key!==key || view.turnActive,transition:view.key===key?view.transition:null,loading:view.key!==key || view.loading,error:view.key===key?view.error:null,onRefresh};
}
