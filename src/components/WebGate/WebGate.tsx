import { useCallback, useEffect, useState, type ReactNode } from "react";
import {
  apiHost,
  currentToken,
  IS_WEB,
  selfHost,
  setApiHost,
  setTokenFor,
  webJson,
} from "../../lib/webApi";

type GateState = "checking" | "need-token" | "ready";

/**
 * Web-mirror entry gate. In the Tauri shell it renders children directly.
 * In a browser it requires a valid API token for the current host before
 * mounting the app: first visit pastes the token from the desktop app
 * (Settings → Web Access), stored per host in localStorage.
 */
export function WebGate({ children }: { children: ReactNode }) {
  const [state, setState] = useState<GateState>(IS_WEB ? "checking" : "ready");
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const validate = useCallback(async () => {
    if (!currentToken()) {
      setState("need-token");
      return;
    }
    try {
      await webJson("GET", "/api/workspaces");
      setState("ready");
    } catch {
      setState("need-token");
      setError("Stored token was rejected — paste a fresh one.");
    }
  }, []);

  useEffect(() => {
    if (IS_WEB) void validate();
  }, [validate]);

  const submit = useCallback(async () => {
    const token = draft.trim();
    if (!token) return;
    setBusy(true);
    setError(null);
    setTokenFor(apiHost(), token);
    try {
      await webJson("GET", "/api/workspaces");
      setState("ready");
    } catch (e) {
      setError(`Token rejected by ${apiHost()}: ${String(e).slice(0, 120)}`);
    } finally {
      setBusy(false);
    }
  }, [draft]);

  if (state === "ready") return <>{children}</>;

  return (
    <div className="flex items-center justify-center h-screen bg-[var(--ct-bg,#0d1117)] text-[var(--ct-fg,#e6edf3)]">
      {state === "checking" ? (
        <div className="text-sm opacity-70">Connecting to {apiHost()}…</div>
      ) : (
        <div className="w-full max-w-md p-6 rounded-lg border border-[var(--ct-border,#30363d)] bg-[var(--ct-titlebar-bg,#161b22)]">
          <h1 className="text-lg font-semibold mb-1">CodeTerm Web</h1>
          <p className="text-xs opacity-70 mb-4">
            Paste the API token for <span className="font-mono">{apiHost()}</span>
            {" — "}find it in the CodeTerm desktop app under Settings → Web
            Access.
          </p>
          <input
            data-testid="web-token-input"
            type="password"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && void submit()}
            placeholder="API token"
            autoFocus
            className="w-full min-h-[44px] px-3 rounded border border-[var(--ct-border,#30363d)] bg-[var(--ct-bg,#0d1117)] text-sm font-mono outline-none focus:border-[var(--ct-accent,#58a6ff)]"
          />
          {error && (
            <div data-testid="web-token-error" className="mt-2 text-xs text-red-400">
              {error}
            </div>
          )}
          <button
            data-testid="web-token-submit"
            onClick={() => void submit()}
            disabled={busy || !draft.trim()}
            className="mt-4 w-full min-h-[44px] rounded bg-[var(--ct-accent,#1f6feb)] text-sm font-medium disabled:opacity-50"
          >
            {busy ? "Checking…" : "Connect"}
          </button>
          {apiHost() !== selfHost() && (
            <button
              data-testid="web-token-back-to-self"
              onClick={() => {
                setApiHost(null);
                window.location.reload();
              }}
              className="mt-2 w-full min-h-[44px] rounded border border-[var(--ct-border,#30363d)] text-xs opacity-80"
            >
              Back to {selfHost()}
            </button>
          )}
        </div>
      )}
    </div>
  );
}
