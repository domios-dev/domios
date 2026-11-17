import {act} from "react";
import {createRoot} from "react-dom/client";
import {expect,it,vi} from "vitest";
import type {Tab} from "../lib/types";
import type {ProviderAccountsSnapshot} from "./providerAccountsTypes";
const fixture=vi.hoisted(()=>({json:vi.fn(),canVerify:false,turnActive:true,continuation:"unverified",status:"ready"}));
vi.mock("../lib/daemonRuntime",()=>({selectedDaemonTransport:{json:fixture.json}}));
vi.mock("../lib/bridge",()=>({listen:async()=>()=>{}}));
vi.mock("./useAgentProviders",()=>({useAgentProviders:()=>({providers:[{id:"fixture",accounts:{},display_name:"Fixture"}],loading:false,error:null})}));
vi.mock("./useProviderAccounts",()=>({useProviderAccounts:()=>({snapshot:{providerId:"fixture",revision:"rendered",continuation:fixture.continuation,accounts:[{id:"default",status:"ready"},{id:"work",status:fixture.status}],workspaceOverrides:[]} as ProviderAccountsSnapshot,error:null})}));
vi.mock("./useAccountTransition",()=>({useAccountTransition:()=>({canVerify:fixture.canVerify,turnActive:fixture.turnActive,error:null,transition:null,onRefresh:()=>{}})}));
import {useTabAccounts} from "./useAccountSelectors";
const tab={id:"own",agent_type:"fixture",session_binding_nonce:"own-nonce",agent_runtime_state:{epoch:9},restorable_launch:{account_id:"default",account_namespace:null}} as unknown as Tab;

async function mounted(){
  Object.assign(globalThis,{IS_REACT_ACT_ENVIRONMENT:true});
  const root=createRoot(document.createElement("div"));let value!:ReturnType<typeof useTabAccounts>;
  function Consumer(){value=useTabAccounts(tab);return null;}
  await act(async()=>root.render(<Consumer/>));
  return {get value(){return value;},close:async()=>{await act(async()=>root.unmount());}};
}
it("uses the owner verification route once and keeps the exact rendered binding",async()=>{
  Object.assign(fixture,{canVerify:true,turnActive:false,continuation:"unverified",status:"ready"});fixture.json.mockReset().mockResolvedValue({});
  const view=await mounted();
  try {await act(async()=>view.value.choose("work"));expect(fixture.json).toHaveBeenCalledExactlyOnceWith("POST","/api/tabs/own/account/verify",expect.objectContaining({nonce:"own-nonce",epoch:9,accountNamespace:null,revision:"rendered",previousAccountId:"default",accountId:"work"}));}
  finally{await view.close();}
});
it.each([{canVerify:false,turnActive:false,status:"ready"},{canVerify:true,turnActive:true,status:"ready"},{canVerify:true,turnActive:false,status:"exhausted"}])("refuses an unverified choice without owner idle eligibility: %j",async values=>{
  Object.assign(fixture,{...values,continuation:"unverified"});fixture.json.mockReset();const view=await mounted();
  try{await act(async()=>{await expect(view.value.choose("work")).rejects.toBeDefined();});expect(fixture.json).not.toHaveBeenCalled();}finally{await view.close();}
});
it("keeps an already proven manual choice on the ordinary safe-boundary route",async()=>{
  Object.assign(fixture,{canVerify:false,turnActive:true,continuation:"proven",status:"ready"});fixture.json.mockReset().mockResolvedValue({});const view=await mounted();
  try{await act(async()=>view.value.choose("work"));expect(fixture.json.mock.calls[0][1]).toBe("/api/tabs/own/account");}finally{await view.close();}
});
