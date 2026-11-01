import {expect,it,vi} from "vitest";
const calls=vi.hoisted(()=>({local:vi.fn(),consent:vi.fn(async()=>true)}));
vi.mock("./daemonBootstrap",()=>({baseDaemonTransport:()=>({localJson:calls.local})}));
vi.mock("./pluginConsentStore",()=>({requestPluginConsent:calls.consent}));
vi.mock("./bridge",()=>({invoke:()=>{throw new Error("selected relay must not receive local document calls");}}));
vi.mock("./pluginSnapshotStore",()=>({ensurePluginSnapshot:()=>{throw new Error("selected snapshot must not authorize local document calls");},runPluginSnapshotAction:()=>{throw new Error("selected relay must not receive consent grants");}}));
import {invokeViewWithConsent} from "./pluginConsent";

it("keeps document permission metadata, grants and callback on its base daemon",async()=>{
  calls.local.mockImplementation(async(method:string)=>method==="GET"?{plugins:[{id:"studio",network_requested:true}]}:{ok:true});
  const operation=vi.fn().mockRejectedValueOnce(new Error("network_grant_required: files.example")).mockResolvedValue({accepted:true});
  expect(await invokeViewWithConsent("studio",operation,()=>true)).toEqual({accepted:true});
  expect(operation).toHaveBeenCalledTimes(2);
  expect(calls.local.mock.calls.map(call=>call.slice(0,2))).toEqual([["GET","/api/plugins/snapshot"],["POST","/api/plugins/action"]]);
  expect(calls.local.mock.calls[1][2]).toMatchObject({pluginId:"studio",permission:"network:files.example",allowed:true});
});
