import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useState } from "react";
import { Check, Copy, KeyRound, Monitor, SquareTerminal } from "lucide-react";
import { useRuntimeLogin } from "./useRuntimeLogin";
import { nativeBridge } from "../../lib/nativeBridge";
import type { AgentRuntimeState } from "../../lib/agentRuntimeState";
import { spawnAiHelper, withAiHelperContext } from "../../lib/aiHelperSpawn";
import { useAiHelperLaunch } from "../../lib/useAiHelperLaunch";
import { ProviderGlyph } from "../ProviderGlyph";
import { BusyState } from "../shared/BusyState";

/**
 * Whether a tab must sign in before it can start: the agent said so (needs_auth), a sign-in is under way, the harness
 * reports it is signed out with a way to sign in, or credentials are missing. An agent that has not reported its state
 * ("unknown") is not asked to sign in: it may already work with a configured key.
 */
export function runtimeSignInRequired(state: AgentRuntimeState | null | undefined): boolean {
  if (!state) return false;
  if (state.state === "loginPending" || state.login?.status === "pending") return true;
  if (state.authentication?.state === "needs_auth") return true;
  if (state.authentication?.state === "authenticated") return false;
  if (state.requiredSecrets?.length) return true;
  return state.auth === "signedOut" && Boolean(state.loginMethods?.length);
}

/** Where the sign-in card goes: centred in an empty chat, a calm notice above the composer once there is a conversation. */
export function signInPlacement(state: AgentRuntimeState | null | undefined, hasConversation: boolean): "center" | "inline" | null {
  if (!runtimeSignInRequired(state)) return null;
  return hasConversation ? "inline" : "center";
}

const method = "inline-flex items-center justify-center gap-1.5 rounded-full border border-[var(--ct-border)] bg-[var(--ct-bg)] px-3.5 py-1.5 text-[13px] text-[var(--ct-fg)] transition-colors hover:bg-[var(--ct-hover)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-[var(--ct-border-focus)] disabled:opacity-50";
const primary = `${method} border-[color-mix(in_srgb,var(--ct-accent)_45%,var(--ct-border))] text-[var(--ct-accent)]`;
const quiet = "inline-flex items-center gap-1.5 rounded-md px-2 py-1 text-[12px] text-[var(--ct-muted-fg)] transition-colors hover:text-[var(--ct-fg)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-border-focus)] disabled:opacity-50";
const field = "min-w-0 flex-1 rounded-full border border-[var(--ct-border)] bg-[var(--ct-bg)] px-3.5 py-1.5 text-[13px] text-[var(--ct-fg)] placeholder:text-[var(--ct-muted-fg)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-border-focus)]";

/**
 * The one sign-in card for any agent tab. `center` sits in the middle of an empty chat; `inline` is a calm notice
 * above the composer once the tab has a conversation. The harness supplies its methods through its runtime state.
 */
export function RuntimeLoginActions({ tabId, state, iconSvg, onCheck, providerId, variant = "inline" }: {
  providerId?: string; tabId: string; state: AgentRuntimeState; iconSvg?: string | null; onCheck?: () => void; variant?: "center" | "inline";
}) {
  const [keyEntry, setKeyEntry] = useState(false);
  const [keyMethod, setKeyMethod] = useState<string | undefined>();
  const [code, setCode] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [copied, setCopied] = useState(false);
  const [sentLogin, setSentLogin] = useState<string | undefined>();
  const { busy, error, operate } = useRuntimeLogin(tabId, providerId);
  const helper = useAiHelperLaunch("sign-in-help-failure");
  const remote = tabId.startsWith("mesh:");
  const expired = state.login?.status === "pending" && state.login.expiresAtMs != null && state.login.expiresAtMs <= Date.now();
  const login = expired ? { ...state.login, status: "failed" as const, url: undefined, userCode: undefined, error: "Sign-in expired. Cancel it and try again." } : state.login;
  const pending = login?.status === "pending";
  const provider = state.loginProviderName ?? state.agentName ?? "the agent";
  const methods = state.loginMethods ?? [];
  const failure = error ?? login?.error;
  const center = variant === "center";
  const openLink = (url: string) => (event: React.MouseEvent) => {
    if (remote) return;
    event.preventDefault();
    void nativeBridge.openUrl(url);
  };
  const title = pending ? login.userCode ? "Enter this code to sign in" : "Finish signing in in your browser" : `Sign in to ${provider}`;
  const hint = pending ? login.userCode ? "Open the sign-in page and enter the code. This tab continues on its own."
    : "This tab continues on its own once you are signed in."
    : state.requiredSecrets?.length ? `Add ${state.requiredSecrets.join(", ")} in Settings › Secrets, then check again.`
    : "Choose how to sign in. The conversation starts once you are signed in.";
  const askGeneral = () => helper.launch(() => spawnAiHelper({ title: "Sign in", description: `Sign in to ${provider}`,
    task: withAiHelperContext(`Help me sign in to ${provider} for a Domios agent tab that is waiting for it. Check what it needs, guide me through it here, then confirm the tab can start. Reply in my language.`) }));

  const actions = !keyEntry && <div className={`flex flex-wrap items-center gap-2 ${center ? "justify-center" : ""}`}>
    {pending ? <>
      {login.url && <a href={login.url} target="_blank" rel="noopener noreferrer" aria-label="Open sign-in page"
        className={primary} onClick={openLink(login.url)}>Open sign-in page</a>}
      {!login.url && <BusyState inline label="Waiting for sign-in" className="px-1 text-[13px] text-[var(--ct-muted-fg)]" />}
      {login.url && <button className={method} type="button" disabled={busy || sentLogin === login.loginId}
        onClick={() => { void operate({ action: "notify" }).then(sent => { if (sent) setSentLogin(login.loginId); }); }}>{sentLogin === login.loginId ? "Notification requested" : "Send to my phone"}</button>}
      <button className={quiet} type="button" disabled={busy || !login.loginId} onClick={() => { void operate({ action: "cancel" }); }}>Cancel</button>
    </> : <>
      {login?.loginId && login.error && <button className={quiet} type="button" disabled={busy} onClick={() => { void operate({action:"cancel"}); }}>Cancel expired sign-in</button>}
      {state.authentication?.methods.map(entry => {
        const takesKey = !remote && Boolean(state.apiKeyMethods?.includes(entry.id));
        return <button key={entry.id} className={method} type="button" disabled={busy || !entry.available}
          onClick={() => { if (takesKey) { setKeyMethod(entry.id); setKeyEntry(true); } else void operate({ methodId: entry.id }); }}>{entry.name}</button>;
      })}
      {state.loginChoices?.map(choice => <button key={choice.id} className={method} type="button" disabled={busy}
        onClick={() => { void operate({ method: "browser", account: choice.id }); }}>{choice.label}</button>)}
      {!state.loginChoices?.length && methods.includes("browser") && <button className={primary} type="button" disabled={busy}
        onClick={() => { void operate({ method: "browser" }); }}>Continue in browser</button>}
      {methods.includes("device") && <button className={method} type="button" disabled={busy}
        onClick={() => { void operate({ method: "device" }); }}><Monitor size={14} aria-hidden="true" />Use a code</button>}
      {!remote && methods.includes("apiKey") && <button className={method} type="button" disabled={busy}
        onClick={() => setKeyEntry(true)}><KeyRound size={14} aria-hidden="true" />Use an API key</button>}
      {!remote && state.loginTerminal && <button className={method} type="button" disabled={busy}
        onClick={() => { void operate({ action: "terminal" }); }}><SquareTerminal size={14} aria-hidden="true" />Use a terminal</button>}
    </>}
  </div>;

  return <section role="group" aria-label={`Sign in to ${provider}`} data-variant={variant}
    className={center
      ? "pointer-events-auto mx-auto flex w-full max-w-[26rem] flex-col items-center gap-3 rounded-2xl border border-[var(--ct-border)] bg-[color-mix(in_srgb,var(--ct-surface)_70%,transparent)] px-6 py-7 text-center"
      : "mx-auto mb-2 flex w-full max-w-4xl flex-col gap-2.5 rounded-xl border border-[var(--ct-border)] bg-[color-mix(in_srgb,var(--ct-surface)_70%,transparent)] px-4 py-3"}>
    <div className={center ? "flex flex-col items-center gap-3" : "flex items-center gap-3"}>
      <span className={`flex shrink-0 items-center justify-center rounded-xl border border-[var(--ct-border)] bg-[var(--ct-bg)] text-[var(--ct-fg)] ${center ? "h-11 w-11 text-[22px]" : "h-8 w-8 text-[16px]"}`}>
        <ProviderGlyph iconSvg={iconSvg} />
      </span>
      <div className="min-w-0">
        <h3 className={`m-0 font-semibold text-[var(--ct-fg)] ${center ? "text-[15px]" : "text-[13px]"}`}>{title}</h3>
        <p className={`m-0 text-[var(--ct-muted-fg)] ${center ? "mt-1 text-[13px]" : "text-[12px]"}`}
          {...(!pending && state.requiredSecrets?.length ? { role: "status", "aria-label": "Required credentials" } : {})}>{hint}</p>
      </div>
    </div>

    {pending && login.userCode && <div className={`flex items-center gap-2 ${center ? "justify-center" : ""}`}>
      <output aria-label="Device code" className="select-all rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)] px-3 py-1.5 font-mono text-xl font-semibold tracking-[0.18em] text-[var(--ct-fg)]">{login.userCode}</output>
      <button type="button" className={quiet} aria-label="Copy device code"
        onClick={() => { void navigator.clipboard.writeText(login.userCode!); setCopied(true); }}>
        {copied ? <Check size={14} aria-hidden="true" /> : <Copy size={14} aria-hidden="true" />}{copied ? "Copied" : "Copy"}
      </button>
    </div>}

    {keyEntry && !pending && <form aria-label="API key sign-in" className="flex w-full flex-wrap items-center gap-2"
      onSubmit={event => { event.preventDefault(); const key = apiKey; const methodId = keyMethod; setApiKey(""); setKeyEntry(false); setKeyMethod(undefined);
        void operate(methodId ? { methodId, apiKey: key } : { method: "apiKey", apiKey: key }); }}>
      <input type="password" aria-label="API key" autoComplete="off" spellCheck={false} placeholder="Paste your API key" autoFocus
        value={apiKey} onChange={event => setApiKey(event.target.value)} className={field} />
      <button className={primary} type="submit" disabled={busy || !apiKey.trim()}>Sign in</button>
      <button className={quiet} type="button" onClick={() => { setApiKey(""); setKeyEntry(false); setKeyMethod(undefined); }}>Cancel</button>
    </form>}

    {actions}

    {pending && login.url && state.loginCodeInput && !remote && <form aria-label="Sign-in code" className="flex w-full items-center gap-2"
      onSubmit={event => { event.preventDefault(); const value = code; setCode(""); void operate({ action: "code", code: value }); }}>
      <input type="password" aria-label="Sign-in code" value={code} autoComplete="off" spellCheck={false}
        onChange={event => setCode(event.target.value)} placeholder="Paste a code if your browser provides one" className={field} />
      <button type="submit" className={primary} disabled={busy || !code.trim()}>Submit code</button>
    </form>}

    {failure && <div className="w-full text-left"><InlineErrorNotice error={failure} operation="signing in" blocking /></div>}

    {!pending && <div className={`flex flex-wrap items-center gap-1 ${center ? "justify-center" : ""}`}>
      {onCheck && <button className={quiet} type="button" disabled={busy} onClick={state.loginChoices?.length ? () => { void operate({ action: "check" }); } : onCheck}>I already signed in</button>}
      <button className={quiet} type="button" disabled={helper.launching} onClick={askGeneral}>{helper.launching ? "Opening…" : "Ask the General agent"}</button>
    </div>}
  </section>;
}
