import { useState } from "react";
import {useRuntimeLogin} from "./useRuntimeLogin";
import { nativeBridge } from "../../lib/nativeBridge";
import type { AgentRuntimeState } from "../../lib/agentRuntimeState";

export function RuntimeLoginActions({tabId,state}: {tabId:string;state:AgentRuntimeState}) {
  const [keyEntry,setKeyEntry] = useState(false);
  const [apiKey,setApiKey] = useState("");
  const {busy,error,operate}=useRuntimeLogin(tabId);
  const remote = tabId.startsWith("mesh:");
  const login = state.login;
  const active = login?.status === "pending";
  const button = "rounded-md border border-[var(--ct-border)] px-2 py-1 text-xs disabled:opacity-50";
  return <div className="flex flex-wrap items-center gap-2" role="group" aria-label="Provider sign-in">
    {active ? <>
      {login.url && <a href={login.url} target="_blank" rel="noopener noreferrer" aria-label="Open sign-in link"
        onClick={remote ? undefined : event => {event.preventDefault();void nativeBridge.openUrl(login.url!);}}>Open sign-in link</a>}
      {login.userCode && <><output aria-label="Device code">{login.userCode}</output><button className={button} type="button" aria-label="Copy device code" onClick={() => {void navigator.clipboard.writeText(login.userCode!);}}>Copy</button></>}
      <button className={button} type="button" disabled={busy || !login.loginId} onClick={() => {void operate({action:"cancel"});}}>Cancel</button>
    </> : <>
      {state.loginMethods?.includes("browser") && <button className={button} type="button" disabled={busy} onClick={() => {void operate({method:"browser"});}}>Sign in to {state.loginProviderName ?? "provider"}</button>}
      {state.loginMethods?.includes("device") && <button className={button} type="button" disabled={busy} onClick={() => {void operate({method:"device"});}}>Use device code</button>}
      {!remote && state.loginMethods?.includes("apiKey") && <button className={button} type="button" disabled={busy} onClick={() => setKeyEntry(true)}>Use API key</button>}
      {keyEntry && <form aria-label="API key sign-in" className="flex items-center gap-2" onSubmit={event => {event.preventDefault();const key=apiKey;setApiKey("");setKeyEntry(false);void operate({method:"apiKey",apiKey:key});}}>
        <input type="password" aria-label="API key" autoComplete="off" value={apiKey} onChange={event => setApiKey(event.target.value)} />
        <button className={button} type="submit" disabled={busy || !apiKey.trim()}>Sign in</button>
        <button className={button} type="button" onClick={() => {setApiKey("");setKeyEntry(false);}}>Cancel</button>
      </form>}
      {!remote && state.loginTerminal && <button className={button} type="button" disabled={busy} onClick={() => {void operate({action:"terminal"});}}>Sign in in terminal</button>}
    </>}
    {(error ?? login?.error) && <span role="alert">{error ?? login?.error}</span>}
  </div>;
}
