import assert from "node:assert/strict";
import {act} from "react";
import {createRoot} from "react-dom/client";
import {Window} from "happy-dom";
import type {UsageSnapshot,RuntimeUsageProjection} from "../../types/usage";
async function main() {
const dom=new Window({url:"http://localhost/"});
Object.defineProperty(globalThis,"IS_REACT_ACT_ENVIRONMENT",{value:true,configurable:true});
for(const key of ["window","document","navigator","HTMLElement","Element","Node","Event","MouseEvent"] as const) Object.defineProperty(globalThis,key,{value:dom[key],configurable:true});
const bridge=await import("../../lib/bridge");
let snapshot!:UsageSnapshot;
const resetRequests:Record<string,unknown>[]=[];
let failResetRead = false;
let groupUsage: unknown[] = [];
let pendingReads: ((snapshot:UsageSnapshot)=>void)[] | null = null;
bridge.__setInvokeForTest(async <T,>(command:string,args?:Record<string,unknown>):Promise<T> => {
 if(command === "agent_runtime_reset_credits") {
  const request = args?.request as Record<string,unknown>;
  resetRequests.push(request);
  if(failResetRead && request.action === "read") throw new Error("opaque background reset read failure");
  return {outcome:"reset"} as T;
 }
 if(command === "usage_latest" && pendingReads) return new Promise<UsageSnapshot>(resolve => pendingReads!.push(resolve)) as Promise<T>;
 return (command === "usage_latest" ? snapshot : command === "provider_model_usage" ? groupUsage : command === "get_config" ? {ui:{usage_intro_hint_opens:5}} : null) as T;
});
const listeners = new Map<string, Set<(event:{payload:unknown})=>void>>();
bridge.__setListenForTest(async <T,>(event:string, handler:(event:{payload:T})=>void) => {
 const handlers = listeners.get(event) ?? new Set();
 listeners.set(event,handlers);
 const listener = handler as (event:{payload:unknown})=>void;
 handlers.add(listener);
 return () => { handlers.delete(listener); };
});
const emit = (provider="fixture") => { for (const listener of listeners.get("usage-snapshot-updated") ?? []) listener({payload:{provider,account_id:null}}); };
const {UsageBubble}=await import("./UsageBubble");
const container=document.createElement("div");document.body.append(container);const root=createRoot(container);
const reading:RuntimeUsageProjection={tab_id:"own",thread_id:"thread",billing_mode:"subscription",token_usage:null,rate_limits:{captured_at:Date.now(),value:[{window:"primary",usedPercent:2},{window:"secondary",usedPercent:0}]}};
async function render(mode:RuntimeUsageProjection){
 snapshot={provider:"fixture",captured_at:Date.now(),runtime_usage:[mode]} as UsageSnapshot;
 await act(async()=>root.render(<UsageBubble key={mode.billing_mode} provider="fixture" tabId="own" threadId="thread" trackingEnabled/>));
}
await render(reading);
assert.equal(container.querySelector("button")?.getAttribute("aria-label"),"usage session 2% weekly 0%");
snapshot = {...snapshot, runtime_usage:[{...reading,rate_limits:{captured_at:Date.now()+1,value:[{window:"primary",usedPercent:7},{window:"secondary",usedPercent:13}]}}]};
await act(async()=>emit("foreign"));
assert.equal(container.querySelector("button")?.getAttribute("aria-label"),"usage session 2% weekly 0%");
await act(async()=>emit());
assert.equal(container.querySelector("button")?.getAttribute("aria-label"),"usage session 7% weekly 13%");
snapshot = {...snapshot, parsed_ok:true, captured_at:Date.now()+2, session_pct:3, weekly_pct:11};
await act(async()=>emit());
assert.equal(container.querySelector("button")?.getAttribute("aria-label"),"usage session 3% weekly 11%");
pendingReads = [];
await act(async()=>emit());
await act(async()=>emit());
assert.equal(pendingReads.length,2);
const newer = {...snapshot,captured_at:snapshot.captured_at+1,weekly_pct:12};
await act(async()=>pendingReads![1](newer));
await act(async()=>pendingReads![0](snapshot));
assert.equal(container.querySelector("button")?.getAttribute("aria-label"),"usage session 3% weekly 12%");
snapshot = newer;
pendingReads = null;
const now = Date.now;
Date.now = () => snapshot.captured_at + 54 * 60_000;
await act(async()=>window.dispatchEvent(new Event("focus")));
assert.ok(container.textContent?.includes("54m ago"));
Date.now = () => snapshot.captured_at + 55 * 60_000;
await act(async()=>document.dispatchEvent(new Event("visibilitychange")));
assert.ok(container.textContent?.includes("55m ago"));
Date.now = now;
await render({...reading,resets:{availableCount:1,nextExpiry:null,credits:null},account_credits:{kind:"balance",balance:"123.45"}});
const failedRefreshContainer=document.createElement("div");document.body.append(failedRefreshContainer);const failedRefreshRoot=createRoot(failedRefreshContainer);
failResetRead = true;
await act(async()=>failedRefreshRoot.render(<UsageBubble provider="fixture" tabId="own" threadId="thread" trackingEnabled/>));
await act(async()=>failedRefreshContainer.querySelector("button")!.click());
assert.deepEqual(resetRequests,[{action:"read"}]);
assert.equal(document.querySelector('[role="alert"]'),null);
assert.equal(document.querySelector('section[role="status"][aria-label]'),null);
assert.equal(failedRefreshContainer.querySelector("button")?.getAttribute("aria-label"),"usage session 2% weekly 0%");
await act(async()=>failedRefreshRoot.unmount());failedRefreshContainer.remove();
failResetRead = false;
resetRequests.length = 0;
await act(async()=>container.querySelector("button")!.click());
assert.deepEqual(resetRequests,[{action:"read"}]);
const namedButton=(name:string)=>{const found=[...document.querySelectorAll("button")].find(button=>(button.getAttribute("aria-label") ?? button.textContent) === name);assert.ok(found);return found;};
await act(async()=>namedButton("Use Session reset").click());
assert.equal(resetRequests.length,1);
const priorCrypto = globalThis.crypto;
Object.defineProperty(globalThis,"crypto",{value:{getRandomValues:priorCrypto.getRandomValues.bind(priorCrypto)},configurable:true});
async function pressReset() {
 const confirmReset = namedButton("Use reset");
 await act(async()=>confirmReset.dispatchEvent(new MouseEvent("pointerdown",{bubbles:true})));
 assert.equal(namedButton("Use reset"),confirmReset);
 assert.ok(document.querySelector('section[aria-label="Session resets"]'));
 await act(async()=>confirmReset.dispatchEvent(new MouseEvent("mousedown",{bubbles:true})));
 await act(async()=>confirmReset.dispatchEvent(new MouseEvent("pointerup",{bubbles:true})));
 await act(async()=>confirmReset.click());
}
await pressReset();
Object.defineProperty(globalThis,"crypto",{value:priorCrypto,configurable:true});
assert.equal(resetRequests.length,3);
assert.deepEqual(resetRequests[2],{action:"read"});
const confirmation:Record<string,unknown> = resetRequests[1];
assert.equal(confirmation.action,"redeem");
assert.equal(confirmation.confirmed,true);
assert.match(String(confirmation.idempotencyKey),/^[0-9a-f-]{36}$/);
assert.ok(document.querySelector('[role="status"]'));
snapshot = {...snapshot,runtime_usage:[{...reading,resets:{availableCount:2,nextExpiry:null,credits:[
  {id:"wire-first",status:"available",expiresAt:null},
  {id:"wire-second",status:"available",expiresAt:null},
]}}]};
await act(async()=>emit());
await act(async()=>namedButton("Use Session reset 2").click());
assert.equal(resetRequests.length,3);
await pressReset();
const selectedConfirmation:Record<string,unknown> = resetRequests[3];
assert.equal(selectedConfirmation.creditId,"wire-second");
assert.equal(selectedConfirmation.confirmed,true);
assert.equal(resetRequests.length,5);
assert.deepEqual(resetRequests[4],{action:"read"});
await render({...reading,billing_mode:"api",token_usage:{captured_at:Date.now(),value:{input_tokens:100,output_tokens:20,cache_read_tokens:30,cost_micros:20000}}});
const button=container.querySelector("button")!;
assert.equal(button.getAttribute("aria-label"),"usage 120 tokens · $0.02");
await act(async()=>button.dispatchEvent(new MouseEvent("click",{bubbles:true})));
assert.match(document.querySelector('[role="status"]')?.textContent ?? "",/cached input: 30/);
const {useProviderModelUsageState} = await import("../../hooks/useProviderModelUsage");
function GroupProbe() { const {usage} = useProviderModelUsageState("fixture"); return <output>{JSON.stringify(usage)}</output>; }
await act(async()=>root.render(<GroupProbe/>));
assert.equal(container.querySelector("output")?.textContent,"[]");
groupUsage = [{group_id:"fixture-group",spend_cents:120}];
await act(async()=>emit("foreign"));
assert.equal(container.querySelector("output")?.textContent,"[]");
await act(async()=>emit());
assert.deepEqual(JSON.parse(container.querySelector("output")!.textContent!),groupUsage);
const expiryNow = Date.UTC(2026, 10, 3);
Date.now = () => expiryNow;
const details = {
  limits:[{kind:"weekly_scoped" as const,label:"Weekly · Fable",percent:47,resets_at:expiryNow+48*3_600_000,is_active:true,critical:true}],
  dollar_credits:[{remaining_dollars:250,resets_at:expiryNow+48*3_600_000}],
  extra_usage:{enabled:false,spend_limit_reached:false,used:12,limit:100,currency:"USD",settings_url:"https://claude.ai/settings/usage"},
};
snapshot = {...snapshot,runtime_usage:undefined,usage_details:details,parsed_ok:true,captured_at:expiryNow,session_pct:17,weekly_pct:60};
await act(async()=>root.render(<UsageBubble key="allowances" provider="fixture" trackingEnabled/>));
await act(async()=>emit());
assert.ok(container.querySelector('[aria-label="Unused credits expire within 3 days"]'));
await act(async()=>container.querySelector("button")!.click());
const dialog = document.querySelector('[role="dialog"][aria-label="usage details"]')!;
assert.ok(dialog.textContent?.includes("Weekly · Fable · active"));
assert.ok(dialog.textContent?.includes("47%"));
assert.ok(dialog.textContent?.includes("$250 credit left · until Nov 5"));
assert.ok(dialog.textContent?.includes("expires in 2 days"));
assert.ok(dialog.textContent?.includes("Usage credits off"));
assert.equal(dialog.querySelector('a')?.getAttribute("href"),"https://claude.ai/settings/usage");
const {nativeBridge} = await import("../../lib/nativeBridge");
const originalOpenUrl = nativeBridge.openUrl;
let openedSettings = "";
nativeBridge.openUrl = async url => {openedSettings = url;};
await act(async()=>dialog.querySelector("a")!.click());
assert.equal(openedSettings,"https://claude.ai/settings/usage");
nativeBridge.openUrl = originalOpenUrl;
assert.equal([...dialog.querySelectorAll("button")].some(button=>button.textContent === "Use"),false);
snapshot = {...snapshot,usage_details:{...details,extra_usage:{...details.extra_usage,enabled:true,effective_enabled:false,disabled_reason:"out_of_credits",decimal_places:2,used:0,limit:10000}}};
await act(async()=>emit());
assert.ok(dialog.textContent?.includes("Usage credits on · no credits left · $100.00/mo limit"));
snapshot = {...snapshot,usage_details:{...details,dollar_credits:[{remaining_dollars:0,resets_at:expiryNow+48*3_600_000}],extra_usage:{...details.extra_usage,enabled:true,spend_limit_reached:true}}};
await act(async()=>emit());
assert.equal(container.querySelector('[aria-label="Unused credits expire within 3 days"]'),null);
assert.ok(dialog.textContent?.includes("Usage credits on · limit reached · $1.00/mo limit"));
assert.equal(dialog.textContent?.includes("expires in"),false);
snapshot = {...snapshot,usage_details:null};
await act(async()=>emit());
assert.equal(dialog.querySelector("a"),null);
assert.equal(dialog.textContent?.includes("Weekly · Fable"),false);
await act(async()=>root.render(<GroupProbe/>));
await render({...reading,resets:{availableCount:1,nextExpiry:expiryNow+3_600_000,credits:[{id:"soon",status:"available",expiresAt:expiryNow+3_600_000}]}});
assert.ok(container.querySelector('[aria-label="Unused credits expire within 3 days"]'));
await act(async()=>container.querySelector("button")!.click());
assert.ok(document.querySelector('[role="dialog"]')?.textContent?.includes("expires in 1 hour"));
Date.now = now;
await act(async()=>root.unmount());bridge.__setInvokeForTest(null);bridge.__setListenForTest(null);dom.happyDOM.abort();
console.log("PASS usage chip wiring: subscription metrics and API session breakdown");

}
void main().catch(error => { console.error(error); process.exitCode=1; });
