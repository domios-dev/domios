import type {RuntimeAuthentication, RuntimeContext, RuntimeSettings} from "@codeterm/plugin-sdk";
import {fail} from "./runtimeProtocol";

export interface PromptClaim {sendId:string;messageId:string;turnId:string}
interface Binding {
  epoch:number; nonce:string; sessionId?:string; instructions:string; settings:RuntimeSettings;
  counter:number; sends:PromptClaim[];
  methods:RuntimeAuthentication["methods"];
}
const bindings = new Map<string,Binding>();
export function binding(ctx:RuntimeContext):Binding {
  const previous = bindings.get(ctx.tabId);
  if (previous?.epoch === ctx.epoch && previous.nonce === ctx.bindingNonce) return previous;
  if (!previous && bindings.size >= 64) fail("size_limit","OpenCode runtime binding capacity reached");
  const state:Binding = {epoch:ctx.epoch,nonce:ctx.bindingNonce,instructions:"",settings:ctx.settings,counter:0,sends:[],methods:[]};
  bindings.set(ctx.tabId,state);
  return state;
}
export function loaded(ctx:RuntimeContext, sessionId:string, instructions:string, settings:RuntimeSettings):void {
  const state = binding(ctx);
  if (instructions.length > 512 * 1024) fail("size_limit","OpenCode instructions exceed the runtime budget");
  state.sessionId = sessionId; state.instructions = instructions; state.settings = settings;
}
export function claimed(ctx:RuntimeContext, claim:PromptClaim):void {
  const state = binding(ctx);
  state.sends = state.sends.filter(send => ctx.claimedSends.some(current => current.sendId === send.sendId));
  if (state.sends.some(send => send.sendId === claim.sendId)) fail("provider_refused","OpenCode prompt handoff already planned; replay refused");
  if (state.sends.length >= 32) fail("size_limit","OpenCode unsettled prompt capacity reached");
  state.sends.push(claim);
}
