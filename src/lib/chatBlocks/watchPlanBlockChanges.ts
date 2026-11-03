import { listen } from "../bridge";
import { onResync } from "../resync";
import type { CommittedPlanChange } from "../../components/Canvas/planProjection";

export function watchPlanBlockChanges(changed: (change?: CommittedPlanChange)=>void): {stop():void;ready:Promise<void>} {
  let active=true;
  const observed=new Map<string,string>();
  const stops:Array<()=>void>=[onResync(()=>{observed.clear();changed();})];
  const pending:Promise<void>[]=[];
  for(const event of ["plan-changed","plans-loaded"]){
    pending.push(listen<CommittedPlanChange>(event,({payload})=>{
      if(!active)return;
      if(event!=="plan-changed" || (typeof payload?.plan_id!=="string" || typeof payload?.revision!=="string" || !/^\d{1,19}$/.test(payload.revision))){changed();return;}
      const before=observed.get(payload.plan_id);
      if(before && BigInt(before)>=BigInt(payload.revision))return;
      observed.delete(payload.plan_id);observed.set(payload.plan_id,payload.revision);
      if(observed.size>128)observed.delete(observed.keys().next().value!);
      changed(payload);
    }).then((off)=>{if(active)stops.push(off);else off();}));
  }
  const reset=()=>{observed.clear();changed();};
  window.addEventListener("codeterm-web-boot-changed",reset);
  return {ready:Promise.all(pending).then(()=>{}),stop:()=>{active=false;for(const off of stops)off();window.removeEventListener("codeterm-web-boot-changed",reset);}};
}
