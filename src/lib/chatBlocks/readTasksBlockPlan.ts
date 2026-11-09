import type { TasksBlockSpec } from "@codeterm/chat-ui";
import { fetchPlanListWindow, fetchPlanProjection, fetchRequestedPlanTaskMetadata, PlanReadError } from "../../components/Canvas/plansApi";
import type { PlanRead } from "../../components/Canvas/planProjection";
import type { ApiConfig } from "../types";
import { webRequestContext, assertWebRequestContext } from "../webApi";

export async function readTasksBlockPlan(api:ApiConfig,spec:TasksBlockSpec,signal:AbortSignal):Promise<PlanRead>{
  const context=webRequestContext();
  let result:PlanRead|undefined;
  const tasks=new Map<string,PlanRead["tasks"][number]>();
  const accept=(page:PlanRead)=>{
    assertWebRequestContext(context);
    if(result && result.revision!==page.revision)throw new Error("Tasks changed while reading this card. Refresh it to read the current tasks.");
    result=page;for(const task of page.tasks)tasks.set(task.id,task);
  };
  if(spec.items){
    const ids=[...new Set(spec.items.map((item)=>item.id))];
    for(let offset=0;offset<ids.length;offset+=25){
      assertWebRequestContext(context);
      accept(await fetchPlanListWindow(api,spec.planId,"all","",signal,undefined,undefined,ids.slice(offset,offset+25),true));
    }
  }else{
    for(const reference of spec.refs){
      assertWebRequestContext(context);
      try{accept(await fetchRequestedPlanTaskMetadata(api,spec.planId,"all",reference,signal));}
      catch(cause){if(cause instanceof PlanReadError && cause.status===404)accept(await fetchPlanProjection(api,spec.planId,"summary",signal));else throw cause;}
    }
    const statuses=spec.groups.length ? spec.groups : spec.refs.length ? [] : ["not_cancelled"];
    for(const status of statuses){assertWebRequestContext(context);accept(await fetchRequestedPlanTaskMetadata(api,spec.planId,status,undefined,signal));}
  }
  if(!result)result=await fetchPlanProjection(api,spec.planId,"summary",signal);
  assertWebRequestContext(context);
  return {...result,tasks:[...tasks.values()]};
}
