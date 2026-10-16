import {useEffect,useRef,useState,type RefObject} from "react";
import type {ChatSessionIntent} from "@codeterm/chat-core";
import {invoke} from "../../lib/bridge";
import {goalMutation,type GoalAction,type GoalSnapshot} from "./goalModel";
export function useSessionGoal(tabId:string,open:boolean,intent:ChatSessionIntent,triggerRef:RefObject<HTMLButtonElement|null>) {
  const [snapshot,setSnapshot]=useState<GoalSnapshot|null>(null);
  const [error,setError]=useState<string|null>(null);
  const [busy,setBusy]=useState(false);
  const [confirming,setConfirming]=useState<{action:GoalAction;snapshot:GoalSnapshot}|null>(null);
  const hadConfirmation=useRef(false);
  useEffect(()=>{
    if (!confirming && hadConfirmation.current) triggerRef.current?.focus();
    hadConfirmation.current=confirming !== null;
  },[confirming,triggerRef]);
  const submitting=useRef(false);
  const currentTab=useRef(tabId);
  currentTab.current=tabId;
  useEffect(()=>{
    setSnapshot(null);
    setConfirming(null);
  },[tabId]);
  useEffect(()=>{
    if (!open) return;
    let active=true;
    setError(null);
    setSnapshot(null);
    void invoke<GoalSnapshot>("agent_runtime_goal",{tabId,request:{action:"read"}}).then(value=>{
      if(active) setSnapshot(value);
    }).catch(()=>{if(active) setError("Could not read the current goal.");});
    return ()=>{active=false;};
  },[open,tabId,intent.goal,intent.goalStatus]);
  const confirm=async()=>{
    if(!confirming || submitting.current) return;
    submitting.current=true;
    setBusy(true);setError(null);
    const target=tabId;
    try {
      const value=await invoke<GoalSnapshot>("agent_runtime_goal",{tabId:target,request:goalMutation(confirming.snapshot,confirming.action)});
      if(currentTab.current===target) {setSnapshot(value);setConfirming(null);}
    } catch {if(currentTab.current===target) setError("Could not update the goal. Refresh the popover and confirm again.");}
    finally {submitting.current=false;setBusy(false);}
  };
  return {snapshot,error,busy,confirming,select:(action:GoalAction)=>{if(snapshot){setError(null);setConfirming({action,snapshot});}},dismiss:()=>{if(!submitting.current){setConfirming(null);setError(null);}},confirm};
}
