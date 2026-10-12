import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
const { request, state } = vi.hoisted(() => ({
  request: vi.fn(), state: {identity:null as null | {uid:string;display_name:string;email:null;provider:string}},
}));
vi.mock("../../lib/webApi", () => ({localFetch: async (url:string) => {
  request(url);
  if (url.endsWith("/login")) state.identity = {uid:"user",display_name:"Ada",email:null,provider:"fake"};
  if (url.endsWith("/logout")) state.identity = null;
  return {ok:true,json:async () => ({identity:state.identity,pending:false})};
}}));
import { useAccount } from "./useAccount";
(globalThis as {IS_REACT_ACT_ENVIRONMENT?:boolean}).IS_REACT_ACT_ENVIRONMENT = true;
function Surface({name}:{name:string}) {
  const {model,signIn,signOut} = useAccount();
  return <section aria-label={name}><output>{model.kind === "signed_in" ? model.identity.uid : model.kind}</output>
    <button type="button" onClick={() => {void signIn();}}>Login</button>
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
    expect(request.mock.calls.filter(([url]) => url.endsWith("/login"))).toHaveLength(1);
    expect(Array.from(host.querySelectorAll("output"), item => item.textContent)).toEqual(["user","user"]);
    await act(async () => buttons[3].click());
    expect(Array.from(host.querySelectorAll("output"), item => item.textContent)).toEqual(["signed_out","signed_out"]);
  } finally {await act(async () => root.unmount());host.remove();}
});
