import { useCallback, useEffect, useRef, useState } from "react";
import type { OperationQuote } from "../../lib/pluginOperationDecision";
import type { ViewHostResult } from "../../lib/pluginViewHostActions";
import { Button } from "../shared/Button";

type Approval = {quote:OperationQuote; phase:"asking"|"recording"|"stale"};

export function usePluginOperationApproval() {
  const [approval,setApproval] = useState<Approval|null>(null);
  const pending = useRef<((accepted:boolean) => void)|null>(null);
  const opened = useRef(false);
  const isOpen = useCallback(() => opened.current,[]);
  const dismiss = useCallback(() => {
    const resolve = pending.current;
    pending.current = null;
    opened.current = false;
    setApproval(null);
    resolve?.(false);
  },[]);
  useEffect(() => () => { pending.current?.(false); pending.current = null; },[]);
  const confirm = useCallback((quote:OperationQuote) => new Promise<boolean>(resolve => {
    pending.current?.(false);
    pending.current = resolve;
    opened.current = true;
    setApproval({quote,phase:"asking"});
  }),[]);
  const approve = useCallback(() => {
    const resolve = pending.current;
    pending.current = null;
    if (!resolve) return;
    setApproval(current => current && {...current,phase:"recording"});
    resolve(true);
  },[]);
  const settled = useCallback((result:ViewHostResult) => {
    if (result.ok) opened.current = false;
    setApproval(current => !current || result.ok ? null : {...current,phase:"stale"});
  },[]);
  return {approval,confirm,approve,dismiss,settled,isOpen};
}

export function PluginOperationApproval({approval,pluginName,onApprove,onDismiss}:{approval:Approval|null;pluginName:string;onApprove:()=>void;onDismiss:()=>void}) {
  const cancel = useRef<HTMLButtonElement>(null);
  useEffect(() => { if (approval?.phase === "asking") cancel.current?.focus(); },[approval]);
  useEffect(() => {
    if (!approval) return;
    const key = (event:KeyboardEvent) => { if (event.key === "Escape") {event.preventDefault();event.stopImmediatePropagation();onDismiss();} };
    window.addEventListener("keydown",key,true);
    return () => window.removeEventListener("keydown",key,true);
  },[approval,onDismiss]);
  if (!approval) return null;
  const amount = approval.quote.amountDisplay;
  return <div role="region" aria-label="Step approval" style={{position:"absolute",bottom:12,left:12,right:12,padding:12,borderRadius:8,background:"var(--ct-bg-elev, var(--ct-surface))",border:"1px solid var(--ct-border-default)",color:"var(--ct-fg)",zIndex:1}}>
    <div role="status">{approval.phase === "stale" ? "The price changed. Check the step again." : `Approve ${amount} for this step?`}</div>
    <div style={{color:"var(--ct-muted-fg)",fontSize:"var(--ct-font-size-sm)",marginTop:4}}>{approval.quote.nodeLabel} · {approval.quote.runLabel} · from {pluginName}</div>
    {approval.quote.aboveRunBudget && <div style={{color:"var(--ct-muted-fg)",fontSize:"var(--ct-font-size-sm)",marginTop:4}}>Above this run's budget. Later steps still stop for approval.</div>}
    <div style={{display:"flex",gap:8,justifyContent:"flex-end",marginTop:8}}>
      {approval.phase !== "stale" && <Button type="button" variant="primary" onClick={onApprove} disabled={approval.phase !== "asking"}>Approve {amount}</Button>}
      <Button ref={cancel} type="button" variant="ghost" onClick={onDismiss}>Not now</Button>
    </div>
  </div>;
}

