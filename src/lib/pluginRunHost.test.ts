import { describe, expect, it, vi } from "vitest";
import { pluginRunHost } from "./pluginRunHost";

describe("plugin run document requests", () => {
  it("uses the document lease path for both run resolution and decisions", async () => {
    const request = vi.fn(async () => ({response:new Response(JSON.stringify({ok:true})),current:() => true}));
    const host = pluginRunHost("fixture",request);
    await host.resolve("a".repeat(64));
    await host.decide({operation:"a".repeat(64),decisionKey:"quoted",action:"preview"});
    expect(request.mock.calls).toEqual([
      ["/api/plugins/run-view",{headers:{"Content-Type":"application/json"},body:JSON.stringify({pluginId:"fixture",reference:"a".repeat(64)})}],
      ["/api/plugins/operation-decision",{headers:{"Content-Type":"application/json"},body:JSON.stringify({pluginId:"fixture",operation:"a".repeat(64),decisionKey:"quoted",action:"preview"})}],
    ]);
  });
  it("refuses a reply retired during body consumption", async () => {
    const host = pluginRunHost("fixture",async () => ({response:new Response(JSON.stringify({ok:true})),current:() => false}));
    expect(await host.resolve("a".repeat(64))).toEqual({ok:false,code:"denied",reason:"stale"});
  });
});
