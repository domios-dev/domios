import { describe, expect, it } from "vitest";
import { acceptChildPage, childHistorySuffix, childTranscriptKey, currentChildAuthority, prependChildPage, type ChildTranscriptPage } from "./childTranscript";

const authority = {rootThreadId:"root",bindingNonce:"nonce",epoch:4};
const page:ChildTranscriptPage = {...authority,child:{threadId:"child",parentThreadId:"root",ordinal:1,status:"completed",prompt:null,model:null,effort:null},items:[],nextCursor:null};
describe("child transcript projection",()=>{
  it("requires a loaded parent with complete binding authority",()=>{
    expect(()=>currentChildAuthority(undefined)).toThrow();
    expect(()=>currentChildAuthority({agent_session_id:"root",session_binding_nonce:"nonce",agent_runtime_state:{state:"loading",epoch:1}})).toThrow();
  });
  it("rejects foreign and stale response authority",()=>{
    expect(acceptChildPage(authority,"child",page)).toBe(true);
    for (const wrong of [{...page,rootThreadId:"foreign"},{...page,bindingNonce:"stale"},{...page,epoch:3},{...page,child:{...page.child,threadId:"foreign"}}]) {
      expect(acceptChildPage(authority,"child",wrong)).toBe(false);
    }
    expect(childTranscriptKey("tab",authority)).not.toBe(childTranscriptKey("tab",{...authority,epoch:5}));
  });
  it("encodes the child and full root authority on a history read",()=>{
    const suffix=childHistorySuffix("child/a",{...authority,cursor:"opaque/&"});
    const url=new URL(suffix,"http://fixture");
    expect(url.pathname).toBe("/agent/runtime/children/child%2Fa/history");
    expect(Object.fromEntries(url.searchParams)).toEqual({...authority,epoch:"4",cursor:"opaque/&"});
  });
  it("prepends older pages without duplicating current rows",()=>{
    expect(prependChildPage([{id:"new",type:"assistant",content:"latest"}], [{id:"old",type:"user",content:"prompt"},{id:"new",type:"assistant",content:"stale"}])).toEqual([
      {id:"old",type:"user",content:"prompt"},{id:"new",type:"assistant",content:"latest"},
    ]);
  });
});
