import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
const { request, rawBody, state } = vi.hoisted(() => ({
  request: vi.fn(), rawBody: vi.fn(() => "opaque-sensitive-response"), state: {
    identity:null as null | {uid:string;display_name:string;email:null;provider:string},
    failure:null as null | {stage:"redemption";error:"provider"},
    failLogin:false, failStatus:false, failPlan:false, failBillingAction:false,
  },
}));
vi.mock("../../lib/webApi", async (importOriginal) => ({
  ...await importOriginal<typeof import("../../lib/webApi")>(),
  localFetch: async (url:string) => {
  request(url);
  if (url.endsWith("/status") && state.failStatus) throw new Error("opaque background status failure");
  if (url.endsWith("/plan") && state.failPlan) throw new Error("opaque background plan failure");
  if (url.endsWith("/portal") && state.failBillingAction) return {ok:false,text:rawBody};
  if (url.split("?")[0].endsWith("/login") && state.failLogin) {
    state.failure = {stage:"redemption",error:"provider"};
    return {ok:false,text:rawBody};
  }
  if (url.endsWith("/plan")) return {ok:true,json:async () => ({entitlements:{plan:"free",status:"active",current_period_end:0,cancel_at_period_end:false},stale:false})};
  if (url.split("?")[0].endsWith("/login")) state.identity = {uid:"user",display_name:"Ada",email:null,provider:"fake"};
  if (url.endsWith("/logout")) state.identity = null;
  return {ok:true,json:async () => ({identity:state.identity,pending:false,last_sign_in_failure:state.failure})};
  },
}));
import { useAccount } from "./useAccount";
import { AccountTab } from "./AccountTab";
(globalThis as {IS_REACT_ACT_ENVIRONMENT?:boolean}).IS_REACT_ACT_ENVIRONMENT = true;
function Surface({name}:{name:string}) {
  const {model,signIn,signOut} = useAccount();
  return <section aria-label={name}><output>{model.kind === "signed_in" ? model.identity.uid : model.kind}</output>
    <button type="button" onClick={() => {void signIn({provider:"email",mode:"signup"});}}>Login</button>
    <button type="button" onClick={() => {void signOut();}}>Logout</button>
  </section>;
}
it("updates both mounted account surfaces after login and logout and guards duplicate login", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<><Surface name="titlebar" /><Surface name="settings" /></>));
    expect(Array.from(host.querySelectorAll("output"), item => item.textContent)).toEqual(["signed_out","signed_out"]);
    const buttons = host.querySelectorAll("button");
    await act(async () => {buttons[0].click();buttons[0].click();});
    expect(request.mock.calls.filter(([url]) => url.split("?")[0].endsWith("/login"))).toHaveLength(1);
    expect(request).toHaveBeenCalledWith("/api/account/login?provider=email&mode=signup");
    expect(Array.from(host.querySelectorAll("output"), item => item.textContent)).toEqual(["user","user"]);
    await act(async () => buttons[3].click());
    expect(Array.from(host.querySelectorAll("output"), item => item.textContent)).toEqual(["signed_out","signed_out"]);
  } finally {await act(async () => root.unmount());host.remove();}
});

it("retains a completed failure across surfaces, focus refresh and remount without reading the error body", async () => {
  state.identity = null; state.failure = null; state.failLogin = true;
  rawBody.mockClear();
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const surfaces = <><Surface name="titlebar" /><Surface name="settings" /></>;
  const kinds = () => Array.from(host.querySelectorAll("output"), item => item.textContent);
  try {
    await act(async () => root.render(surfaces));
    await act(async () => host.querySelector("button")!.click());
    expect(kinds()).toEqual(["error", "error"]);
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(kinds()).toEqual(["error", "error"]);
    await act(async () => root.render(null));
    await act(async () => root.render(surfaces));
    expect(kinds()).toEqual(["error", "error"]);
    expect(rawBody).not.toHaveBeenCalled();
  } finally {
    await act(async () => root.unmount()); host.remove();
    state.identity = null; state.failure = null; state.failLogin = false;
  }
});

it("keeps failed background account and plan reads quiet while preserving user action errors", async () => {
  state.identity = {uid:"user",display_name:"Ada",email:null,provider:"fake"};
  state.failure = null;
  state.failLogin = false;
  state.failStatus = false;
  state.failPlan = true;
  state.failBillingAction = false;
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AccountTab />));
    await act(async () => { await Promise.resolve(); });
    expect(host.textContent).toContain("UID: user");
    expect(host.querySelector('[role="alert"]')).toBeNull();

    state.failStatus = true;
    await act(async () => window.dispatchEvent(new Event("focus")));
    await act(async () => { await Promise.resolve(); });
    expect(host.textContent).toContain("UID: user");
    expect(host.querySelector('[role="alert"]')).toBeNull();

    state.failBillingAction = true;
    const manage = Array.from(host.querySelectorAll("button")).find(button => button.textContent === "Manage subscription");
    expect(manage).toBeDefined();
    await act(async () => manage!.click());
    expect(host.querySelectorAll('[role="alert"]')).toHaveLength(1);
  } finally {
    await act(async () => root.unmount());
    host.remove();
    state.identity = null; state.failure = null; state.failLogin = false; state.failStatus = false; state.failPlan = false; state.failBillingAction = false;
  }
});

it("shows a direct sign-in failure in one accessible alert", async () => {
  state.identity = null; state.failure = null; state.failLogin = true; state.failStatus = false; state.failPlan = false; state.failBillingAction = false;
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AccountTab />));
    const signIn = Array.from(host.querySelectorAll("button")).find(button => button.textContent === "Sign in");
    expect(signIn).toBeDefined();
    await act(async () => signIn!.click());
    expect(host.querySelectorAll('[role="alert"]')).toHaveLength(1);
  } finally {
    await act(async () => root.unmount());
    host.remove();
    state.identity = null; state.failure = null; state.failLogin = false;
  }
});
