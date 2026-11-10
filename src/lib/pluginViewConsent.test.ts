import {expect,it,vi} from "vitest";
const calls=vi.hoisted(()=>({local:vi.fn(),consent:vi.fn(async()=>true)}));
vi.mock("./daemonBootstrap",()=>({baseDaemonTransport:()=>({localJson:calls.local,origin:()=>"http://local.fixture",currentToken:()=>"local-token"})}));
vi.mock("./pluginConsentStore",()=>({requestPluginConsent:calls.consent}));
vi.mock("./bridge",()=>({invoke:()=>{throw new Error("selected relay must not receive local document calls");}}));
vi.mock("./pluginSnapshotStore",()=>({ensurePluginSnapshot:()=>{throw new Error("selected snapshot must not authorize local document calls");},runPluginSnapshotAction:()=>{throw new Error("selected relay must not receive consent grants");}}));
import {invokeViewWithConsent} from "./pluginConsent";
import {readPluginSnapshot} from "./pluginSnapshotRequest";

it("keeps document permission metadata, grants and callback on its base daemon",async()=>{
  calls.local.mockImplementation(async(method:string)=>method==="GET"?{plugins:[{id:"studio",network_requested:true}]}:{ok:true});
  const operation=vi.fn().mockRejectedValueOnce(new Error("network_grant_required: files.example")).mockResolvedValue({accepted:true});
  expect(await invokeViewWithConsent("studio",operation,()=>true)).toEqual({accepted:true});
  expect(operation).toHaveBeenCalledTimes(2);
  expect(calls.local.mock.calls.map(call=>call.slice(0,2))).toEqual([["GET","/api/plugins/snapshot"],["POST","/api/plugins/action"]]);
  expect(calls.local.mock.calls[1][2]).toMatchObject({pluginId:"studio",permission:"network:files.example",allowed:true});
});

it("shares concurrent local consent and catalogue reads without crossing daemon or relay authority",async()=>{
  let finish!: (value: {plugins: Array<{id: string}>}) => void;
  const snapshot=new Promise<{plugins: Array<{id: string}>}>(resolve=>{finish=resolve;});
  calls.local.mockClear();
  calls.local.mockImplementation(()=>snapshot);
  const client={json:calls.local,localJson:calls.local,origin:()=>"http://local.fixture",currentToken:()=>"local-token"};
  const catalogue=readPluginSnapshot(client);
  const operation=vi.fn(async()=>({ok:true}));
  const consent=invokeViewWithConsent("studio",operation,()=>true);
  await Promise.resolve();
  expect(calls.local).toHaveBeenCalledTimes(1);
  finish({plugins:[{id:"studio"}]});
  await catalogue;
  expect(await consent).toEqual({ok:true});
  expect(operation).toHaveBeenCalledTimes(1);

  calls.local.mockClear();
  await Promise.all([
    readPluginSnapshot(client),
    readPluginSnapshot({...client,currentToken:()=>"other-token"}),
    readPluginSnapshot({...client,origin:()=>"http://other.fixture"}),
    readPluginSnapshot(client,{peerId:()=>"peer"}),
  ]);
  expect(calls.local).toHaveBeenCalledTimes(4);
});
