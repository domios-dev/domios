import { useState } from "react";
import { Check, Copy, KeyRound, Monitor, SquareTerminal } from "lucide-react";
import { useRuntimeLogin } from "./useRuntimeLogin";
import { nativeBridge } from "../../lib/nativeBridge";
import type { AgentRuntimeState } from "../../lib/agentRuntimeState";
import { ProviderGlyph } from "../ProviderGlyph";
import { BusyState } from "../shared/BusyState";

const primary = "inline-flex items-center gap-2 rounded-md bg-[var(--ct-accent)] px-3 py-1.5 text-sm font-medium text-[var(--ct-bg)] transition-opacity hover:opacity-90 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ct-border-focus)] disabled:opacity-50";
const quiet = "inline-flex items-center gap-1.5 rounded-md px-2 py-1.5 text-sm text-[var(--ct-muted-fg)] transition-colors hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-border-focus)] disabled:opacity-50";

export function RuntimeLoginActions({ tabId, state, iconSvg, onCheck }: {
  tabId: string; state: AgentRuntimeState; iconSvg?: string | null; onCheck?: () => void;
}) {
  const [keyEntry, setKeyEntry] = useState(false);
  const [apiKey, setApiKey] = useState("");
  const [copied, setCopied] = useState(false);
  const { busy, error, operate } = useRuntimeLogin(tabId);
  const remote = tabId.startsWith("mesh:");
  const login = state.login;
  const pending = login?.status === "pending";
  const provider = state.loginProviderName ?? "the provider";
  const methods = state.loginMethods ?? [];
  const failure = error ?? login?.error;
  const openLink = (url: string) => (event: React.MouseEvent) => {
    if (remote) return;
    event.preventDefault();
    void nativeBridge.openUrl(url);
  };
  const title = pending ? login.userCode ? "Enter this code to sign in" : "Finish signing in in your browser" : `Sign in to ${provider}`;
  const hint = pending ? login.userCode ? "Open the sign-in page and enter the code. This tab continues on its own."
    : "This tab continues on its own once you are signed in." : "This tab needs your account before it can start.";

  return <section role="group" aria-label={`Sign in to ${provider}`}
    className="mx-auto my-2 w-full max-w-xl rounded-lg border border-[var(--ct-border)] bg-[var(--ct-surface)] px-4 py-3">
    <div className="flex items-start gap-3">
      <span className="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-md border border-[var(--ct-border)] text-[16px] text-[var(--ct-fg)]">
        <ProviderGlyph iconSvg={iconSvg} />
      </span>
      <div className="min-w-0 flex-1">
        <h3 className="text-sm font-semibold text-[var(--ct-fg)]">{title}</h3>
        <p className="mt-0.5 text-sm text-[var(--ct-muted-fg)]">{hint}</p>

        {pending && login.userCode && <div className="mt-3 flex items-center gap-2">
          <output aria-label="Device code" className="select-all rounded-md border border-[var(--ct-border)] bg-[var(--ct-bg)] px-3 py-1.5 font-mono text-xl font-semibold tracking-[0.18em] text-[var(--ct-fg)]">{login.userCode}</output>
          <button type="button" className={quiet} aria-label="Copy device code"
            onClick={() => { void navigator.clipboard.writeText(login.userCode!); setCopied(true); }}>
            {copied ? <Check size={15} aria-hidden="true" /> : <Copy size={15} aria-hidden="true" />}{copied ? "Copied" : "Copy"}
          </button>
        </div>}

        {keyEntry && !pending && <form aria-label="API key sign-in" className="mt-3 flex flex-wrap items-center gap-2"
          onSubmit={event => { event.preventDefault(); const key = apiKey; setApiKey(""); setKeyEntry(false); void operate({ method: "apiKey", apiKey: key }); }}>
          <input type="password" aria-label="API key" autoComplete="off" spellCheck={false} placeholder="Paste your API key" autoFocus
            value={apiKey} onChange={event => setApiKey(event.target.value)}
            className="min-w-0 flex-1 rounded-md border border-[var(--ct-border)] bg-[var(--ct-bg)] px-2.5 py-1.5 text-sm text-[var(--ct-fg)] placeholder:text-[var(--ct-muted-fg)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-border-focus)]" />
          <button className={primary} type="submit" disabled={busy || !apiKey.trim()}>Sign in</button>
          <button className={quiet} type="button" onClick={() => { setApiKey(""); setKeyEntry(false); }}>Cancel</button>
        </form>}

        {!keyEntry && <div className="mt-3 flex flex-wrap items-center gap-1">
          {pending ? <>
            {login.url && <a href={login.url} target="_blank" rel="noopener noreferrer" aria-label="Open sign-in page"
              className={primary} onClick={openLink(login.url)}>Open sign-in page</a>}
            {!login.url && <BusyState inline label="Waiting for sign-in" className="px-1 text-sm text-[var(--ct-muted-fg)]" />}
            <button className={quiet} type="button" disabled={busy || !login.loginId} onClick={() => { void operate({ action: "cancel" }); }}>Cancel</button>
          </> : <>
            {methods.includes("browser") && <button className={primary} type="button" disabled={busy}
              onClick={() => { void operate({ method: "browser" }); }}>Continue in browser</button>}
            {methods.includes("device") && <button className={quiet} type="button" disabled={busy}
              onClick={() => { void operate({ method: "device" }); }}><Monitor size={15} aria-hidden="true" />Use a code</button>}
            {!remote && methods.includes("apiKey") && <button className={quiet} type="button" disabled={busy}
              onClick={() => setKeyEntry(true)}><KeyRound size={15} aria-hidden="true" />Use an API key</button>}
            {!remote && state.loginTerminal && <button className={quiet} type="button" disabled={busy}
              onClick={() => { void operate({ action: "terminal" }); }}><SquareTerminal size={15} aria-hidden="true" />Use a terminal</button>}
            {onCheck && <button className={`${quiet} ml-auto`} type="button" disabled={busy} onClick={onCheck}>I already signed in</button>}
          </>}
        </div>}

        {failure && <p role="alert" className="mt-2 text-sm text-[var(--ct-red)]">{failure}</p>}
      </div>
    </div>
  </section>;
}
