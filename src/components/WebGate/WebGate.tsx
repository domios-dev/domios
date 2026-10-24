import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useCallback, useEffect, useState, type ReactNode } from "react";
import {
  apiHost,
  clearRelayTarget,
  currentToken,
  IS_WEB,
  localJson,
  relayTarget,
  selfHost,
  setRelayTarget,
  setTokenFor,
  usableRelayTarget,
  WebApiError,
} from "../../lib/webApi";
import type { MeshPeersResponse } from "../../lib/types";
import { releaseBootCover } from "../../lib/bootCover";
import { installWebHistoryGuard } from "./webHistoryGuard";
import { consumeWebEntry } from "./webEntry";

type GateState = "checking" | "need-token" | "ready";

/**
 * Web-mirror entry gate. In the Tauri shell it renders children directly.
 * In a browser it requires a valid local-daemon API token before mounting
 * the app: first visit pastes the token from the desktop app (Settings ->
 * Web Access), stored in localStorage. Mesh peers are then viewed through the
 * relay (see WebPeerPicker) using this same local token — no per-peer token.
 */
export function WebGate({ children }: { children: ReactNode }) {
  const [state, setState] = useState<GateState>(IS_WEB ? "checking" : "ready");
  useEffect(() => {
    if (state !== "ready" && state !== "checking") releaseBootCover();
  }, [state]);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const validate = useCallback(async (isActive: () => boolean = () => true) => {
    if (!currentToken()) {
      setState("need-token");
      return;
    }
    try {
      // The token authenticates the browser to its OWN daemon, so probe it there.
      // A relayed probe fails for peer reasons and would blame the token for them.
      await localJson("GET", "/api/workspaces");
      if (isActive()) setState("ready");
    } catch (error) {
      if (!isActive()) return;
      setState("need-token");
      setError(
        error instanceof WebApiError && (error.status === 401 || error.status === 403)
          ? "Stored token was rejected — paste a fresh one."
          : "Cannot reach the Domios daemon on this host.",
      );
    }
  }, []);

  useEffect(() => {
    if (!IS_WEB) return;
    let generation = 0;
    let disposed = false;
    const navigate = (initial = false) => {
      const entry = consumeWebEntry(window.location);
      if (!initial && !entry.hasEntry) return;
      const current = ++generation;
      const active = () => !disposed && generation === current;
      if (entry.hasEntry) {
        window.history.replaceState(window.history.state, "", entry.cleanUrl);
      }
      if (entry.token) setTokenFor(apiHost(), entry.token);
      const relayPeerId = entry.relay;
      setError(null);
      const bootstrap = async () => {
        const stored = relayTarget();
        if (relayPeerId === "self") {
          clearRelayTarget();
        } else if (relayPeerId) {
          try {
            const peers = await localJson<MeshPeersResponse>("GET", "/api/mesh/peers");
            const peer = usableRelayTarget(peers.paired, relayPeerId);
            if (!peer) throw new Error("Relay peer cannot serve as a relay target");
            if (active()) setRelayTarget(peer.id, peer.name);
          } catch {
            if (active()) clearRelayTarget();
          }
        } else if (stored) {
          // A target left in localStorage outlives pairing, presence and protocol;
          // adopting it unchecked relays every request into a dead peer.
          try {
            const peers = await localJson<MeshPeersResponse>("GET", "/api/mesh/peers");
            if (active() && !usableRelayTarget(peers.paired, stored.id)) clearRelayTarget();
          } catch {
            /* local daemon unreachable — validate() reports that, keep the target */
          }
        }
        if (active()) await validate(active);
      };
      void bootstrap();
    };
    const onNavigate = () => navigate();
    window.addEventListener("hashchange", onNavigate);
    window.addEventListener("popstate", onNavigate);
    navigate(true);
    const removeHistoryGuard = installWebHistoryGuard();
    return () => {
      disposed = true;
      window.removeEventListener("hashchange", onNavigate);
      window.removeEventListener("popstate", onNavigate);
      removeHistoryGuard();
    };
  }, [validate]);

  const submit = useCallback(async () => {
    const token = draft.trim();
    if (!token) return;
    setBusy(true);
    setError(null);
    setTokenFor(apiHost(), token);
    try {
      await localJson("GET", "/api/workspaces");
      setState("ready");
    } catch (e) {
      setError(
        e instanceof WebApiError && (e.status === 401 || e.status === 403)
          ? `Token rejected by ${apiHost()}`
          : `${apiHost()} did not answer: ${String(e).slice(0, 120)}`,
      );
    } finally {
      setBusy(false);
    }
  }, [draft]);

  if (state === "ready") return children;

  return (
    <div className="flex items-center justify-center h-screen bg-[var(--ct-bg,#0d1117)] text-[var(--ct-fg,#e6edf3)]">
      {state === "checking" ? (
        <div className="text-sm opacity-70">Connecting to {apiHost()}…</div>
      ) : (
        <div className="w-full max-w-md p-6 rounded-lg border border-[var(--ct-border,#30363d)] bg-[var(--ct-titlebar-bg,#161b22)]">
          <h1 className="text-lg font-semibold mb-1">Domios Web</h1>
          <p className="text-xs opacity-70 mb-4">
            Paste the API token for <span className="font-mono">{apiHost()}</span>
            {" — "}find it in the Domios desktop app under Settings {"->"} Web
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
            <div data-testid="web-token-error"><InlineErrorNotice error={error} operation="using web gate" /></div>
          )}
          <button
            data-testid="web-token-submit"
            onClick={() => void submit()}
            disabled={busy || !draft.trim()}
            className="mt-4 w-full min-h-[44px] rounded bg-[var(--ct-accent,#1f6feb)] text-sm font-medium disabled:opacity-50"
          >
            {busy ? "Checking…" : "Connect"}
          </button>
          {relayTarget() && (
            <button
              data-testid="web-token-back-to-self"
              onClick={() => {
                clearRelayTarget();
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
