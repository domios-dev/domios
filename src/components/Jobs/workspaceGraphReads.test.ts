import { describe, expect, it, vi } from "vitest";
import { attachment, deferred, fixtureGraph } from "../Automation/fixtures.testdata";
import type { AutomationAttachment, AutomationGraph } from "../Automation/types";
import { createWorkspaceGraphReads } from "./workspaceGraphReads";

describe("Jobs workspace graph reads", () => {
  it("loads every workspace with at most three reads in flight", async () => {
    const scopes = Array.from({length:64}, (_, index) => ({...attachment,workspace_id:`workspace-${index}`}));
    const starts = scopes.map(() => deferred<void>());
    const reads = scopes.map(() => deferred<AutomationGraph>());
    let active = 0, peak = 0, issued = 0;
    const load = vi.fn(async (scope: AutomationAttachment) => {
      const index=issued++;
      active++; peak=Math.max(peak,active); starts[index].resolve();
      try { return await reads[index].promise; }
      finally { active--; }
    });
    const read=createWorkspaceGraphReads(load);
    const results=scopes.map(scope=>read(scope,new AbortController().signal));
    expect(load).toHaveBeenCalledTimes(3);
    for (let index=0;index<scopes.length;index++) {
      await starts[index].promise;
      expect(load.mock.calls[index][0]).toBe(scopes[index]);
      reads[index].resolve({...fixtureGraph(),workspace_id:scopes[index].workspace_id});
    }
    expect((await Promise.all(results)).map(graph=>graph.workspace_id)).toEqual(scopes.map(scope=>scope.workspace_id));
    expect(peak).toBe(3);
  });

  it("cancels queued reads before dispatch and advances after an active read fails", async () => {
    const controllers=Array.from({length:5},()=>new AbortController());
    const held=Array.from({length:5},()=>deferred<AutomationGraph>());
    const fifthStarted=deferred<void>();
    const load=vi.fn((scope: AutomationAttachment,signal:AbortSignal)=>{
      const index=Number(scope.workspace_id);
      if(index===4)fifthStarted.resolve();
      const abort=()=>held[index].reject(signal.reason);
      signal.addEventListener("abort",abort,{once:true});
      return held[index].promise.finally(()=>signal.removeEventListener("abort",abort));
    });
    const read=createWorkspaceGraphReads(load);
    const results=controllers.map((controller,index)=>read({...attachment,workspace_id:String(index)},controller.signal).catch(error=>error));
    controllers[3].abort();
    expect(await results[3]).toMatchObject({name:"AbortError"});
    expect(load.mock.calls.map(([scope])=>scope.workspace_id)).toEqual(["0","1","2"]);
    controllers[0].abort();
    expect(await results[0]).toMatchObject({name:"AbortError"});
    await fifthStarted.promise;
    expect(load.mock.calls.map(([scope])=>scope.workspace_id)).toEqual(["0","1","2","4"]);
    for(const index of [1,2,4])held[index].resolve(fixtureGraph());
    await Promise.all(results);
  });
});
