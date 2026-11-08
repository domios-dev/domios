import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { AgentRuntimeBanner } from "./AgentRuntimeBanner";
import { RuntimeLoginActions, runtimeSignInRequired, signInPlacement } from "./RuntimeLoginActions";
import { invoke } from "../../lib/bridge";

vi.mock("../../lib/bridge", () => ({invoke:vi.fn().mockResolvedValue(undefined)}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("opens the accessible runtime badge and wires Retry once, with outside and Escape dismissal", async () => {
  const retry = vi.fn();
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AgentRuntimeBanner state={{ state: "degraded", epoch: 1, reason: "opaque-fault" }} onRetry={retry} />));
    const badge = host.querySelector<HTMLButtonElement>('button[aria-haspopup="dialog"]')!;
    expect(badge).not.toBeNull();
    expect(badge.getAttribute("aria-label")).toBeTruthy();
    await act(async () => badge.click());
    expect(document.querySelector('[role="dialog"]')).not.toBeNull();
    const control = document.querySelector<HTMLButtonElement>('button[aria-label="Retry"]')!;
    expect(control).not.toBeNull();
    await act(async () => control.click());
    expect(retry).toHaveBeenCalledTimes(1);
    await act(async () => document.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true })));
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(document.activeElement).toBe(badge);
    await act(async () => badge.click());
    await act(async () => document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true })));
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    await act(async () => badge.click());
    await act(async () => root.render(<AgentRuntimeBanner state={{ state: "loaded", epoch: 2 }} onRetry={retry} />));
    expect(badge.disabled).toBe(true);
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    await act(async () => root.render(<AgentRuntimeBanner state={{ state: "degraded", epoch: 3, reason: "another-fault" }} onRetry={retry} />));
    expect(host.querySelector('button[aria-haspopup="dialog"]')).toBe(badge);
    expect(badge.disabled).toBe(false);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});


it("offers history Retry while the conversation runtime is loaded", async () => {
  const retry=vi.fn(), host=document.createElement("div");document.body.append(host);
  const root=createRoot(host);
  try {
    await act(async()=>root.render(<AgentRuntimeBanner state={{state:"loaded",epoch:2,history:"failed",historyPages:8,historyError:"fixture-page-nine"}} onRetry={retry} />));
    const badge=host.querySelector<HTMLButtonElement>('button[aria-haspopup="dialog"]')!;
    expect(badge.getAttribute("aria-label")).toBeTruthy();
    await act(async()=>badge.click());
    const control=document.querySelector<HTMLButtonElement>('button[aria-label="Retry"]')!;
    expect(control.disabled).toBe(false);
    await act(async()=>control.click());expect(retry).toHaveBeenCalledTimes(1);
  } finally { await act(async()=>root.unmount());host.remove(); }
});

it("offers one fresh-conversation action for a missing saved rollout", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const retry = vi.fn();
  try {
    await act(async () => root.render(<AgentRuntimeBanner tabId="opaque-tab" state={{state:"degraded",epoch:2,reason:"ConversationMissing: opaque diagnostic"}} onRetry={retry} />));
    await act(async () => host.querySelector<HTMLButtonElement>('button[aria-haspopup="dialog"]')!.click());
    const start = document.querySelector<HTMLButtonElement>('button[aria-label="Start a new conversation"]');
    expect(start).not.toBeNull();
    expect(document.querySelector('button[aria-label="Retry"]')).toBeNull();
    await act(async () => start!.click());
    expect(invoke).toHaveBeenCalledWith("restart_agent_tab",{tabId:"opaque-tab",fresh:true});
    expect(retry).not.toHaveBeenCalled();
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("shows the sign-in card for a launch blocked on a profile secret before an agent reports auth", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); const retry = vi.fn();
  const state = {state:"loginPending" as const,epoch:1,auth:"unknown" as const,loginProviderName:"Fixture",requiredSecrets:["domios:provider:fixture:key"]};
  try {
    expect(runtimeSignInRequired(state)).toBe(true);
    // The banner in the composer no longer carries the card; the chat area does.
    await act(async () => root.render(<AgentRuntimeBanner tabId="owned" state={state} onRetry={retry} />));
    expect(host.querySelector('[role="group"][aria-label="Sign in to Fixture"]')).toBeNull();
    await act(async () => root.render(<RuntimeLoginActions tabId="owned" state={state} onCheck={retry} variant="center" />));
    expect(host.querySelector('[role="group"][aria-label="Sign in to Fixture"]')).not.toBeNull();
    expect(host.querySelector('[role="status"][aria-label="Required credentials"]')?.textContent).toContain("domios:provider:fixture:key");
    const check = Array.from(host.querySelectorAll("button")).find(button => button.textContent === "I already signed in");
    await act(async () => check!.click()); expect(retry).toHaveBeenCalledTimes(1);
  } finally {await act(async () => root.unmount()); host.remove();}
});

it("asks for a sign-in only when the agent needs one, never for an unreported (unknown) state", () => {
  for (const state of ["loaded", "degraded"] as const) {
    expect(runtimeSignInRequired({state,epoch:1,auth:"unknown",authentication:{state:"unknown",methods:[]},requiredSecrets:[]})).toBe(false);
    expect(runtimeSignInRequired({state,epoch:1,authentication:{state:"unknown",methods:[{id:"google",name:"Log in with Google",available:true}]}})).toBe(false);
  }
  expect(runtimeSignInRequired({state:"loaded",epoch:1,authentication:{state:"authenticated",methods:[]}})).toBe(false);
  expect(runtimeSignInRequired({state:"loaded",epoch:1,authentication:{state:"needs_auth",methods:[]}})).toBe(true);
  expect(runtimeSignInRequired({state:"loaded",epoch:1,auth:"signedOut",loginMethods:["browser"]})).toBe(true);
});

it("shows one typed failure sentence with collapsed technical details", async () => {
  const host=document.createElement("div"); document.body.append(host);
  const root=createRoot(host);
  const reason='/api/tabs/opaque/chat-raw/start → HTTP 400: '+JSON.stringify({error:{code:"provider_refused",message:"The session is already active.",sessionId:"opaque-session"}});
  try {
    await act(async()=>root.render(<AgentRuntimeBanner tabId="owned" state={{state:"degraded",epoch:1,reason}} onRetry={vi.fn()} />));
    await act(async()=>host.querySelector<HTMLButtonElement>('button[aria-haspopup="dialog"]')!.click());
    const notice=document.querySelector('[role="status"][aria-label="The session is already active."]');
    expect(notice).not.toBeNull();
    const details=notice!.querySelector("details")!;
    expect(details.open).toBe(false);
    expect(details.querySelector("summary")?.textContent).toBe("Details");
    expect(details.textContent).toContain("HTTP 400");
    expect(notice!.querySelector("p")?.textContent).toBe("The session is already active.");
    await act(async()=>root.render(<AgentRuntimeBanner tabId="owned" state={{state:"degraded",epoch:1,reason:JSON.stringify({error:{code:"provider_refused",message:"Provider refused: ProviderRefused: opaque"}})}} onRetry={vi.fn()} />));
    expect(document.querySelector('[role="status"][aria-label="The agent refused this operation."]')).not.toBeNull();

  } finally {await act(async()=>root.unmount());host.remove();}
});

it("places the sign-in card in the middle of an empty chat and above the composer once there is a conversation", () => {
  const needs = {state:"loaded" as const,epoch:1,authentication:{state:"needs_auth" as const,methods:[]}};
  expect(signInPlacement(needs,false)).toBe("center");
  expect(signInPlacement(needs,true)).toBe("inline");
  expect(signInPlacement({state:"loaded",epoch:1,authentication:{state:"unknown",methods:[]}},false)).toBeNull();
});
