// Bitwarden connection panel — the plugin's own UI, rendered in the host's
// sandboxed iframe. Talks to the plugin (its secret methods) via window.ct.
import { useCallback, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";

type Tone = "ok" | "warn" | "danger" | "muted";
declare global {
  interface Window {
    ct?: { invoke(method: string, args?: unknown): Promise<any> };
  }
}
const ct = () => window.ct!;

const COLOR: Record<Tone, string> = {
  ok: "var(--ct-ok, #4caf50)",
  warn: "var(--ct-warn, #e0a030)",
  danger: "var(--ct-err, #e57373)",
  muted: "var(--ct-muted, #9aa)",
};

function Badge({ tone, label }: { tone: Tone; label: string }) {
  const c = COLOR[tone];
  return (
    <span style={{ fontSize: 11, fontWeight: 560, padding: "2px 9px", borderRadius: 999, color: c, background: `color-mix(in srgb, ${c} 16%, transparent)`, border: `1px solid color-mix(in srgb, ${c} 35%, transparent)` }}>
      {label}
    </span>
  );
}

const inputStyle: React.CSSProperties = {
  width: "100%", boxSizing: "border-box", background: "var(--ct-bg-elev, rgba(255,255,255,0.03))",
  border: "1px solid var(--ct-border-default, rgba(255,255,255,0.12))", borderRadius: 6,
  padding: "7px 9px", color: "var(--ct-fg, #eee)", fontSize: 12.5,
};
const btnStyle: React.CSSProperties = {
  font: "inherit", fontSize: 12.5, fontWeight: 540, padding: "6px 14px", borderRadius: 6,
  border: "none", background: "var(--ct-accent, #5b8cff)", color: "#fff", cursor: "pointer", whiteSpace: "nowrap",
};

interface Status {
  status: "unlocked" | "locked" | "logged_out" | "unavailable";
  user?: string | null;
  endpoint?: string | null;
  reason?: string | null;
}

function App() {
  const [status, setStatus] = useState<Status | null>(null);
  const [serverUrl, setServerUrl] = useState("");
  const [email, setEmail] = useState("");
  const [master, setMaster] = useState("");
  const [twoFactor, setTwoFactor] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [s, su] = await Promise.all([ct().invoke("status"), ct().invoke("serverUrl")]);
      setStatus(s as Status);
      setServerUrl((su as { url?: string })?.url ?? "");
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const st = status?.status;
  const needsLogin = st !== "locked";
  const canSubmit = !!master && (!needsLogin || !!email.trim());

  const run = useCallback(async (fn: () => Promise<unknown>, errPrefix: string) => {
    setBusy(true);
    setError(null);
    try {
      const r: any = await fn();
      if (r && r.error) throw new Error(r.error.message || r.error.kind || String(r.error));
      await refresh();
    } catch (e) {
      setError(`${errPrefix}: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setBusy(false);
    }
  }, [refresh]);

  return (
    <div style={{ fontFamily: "system-ui, sans-serif", padding: 18, color: "var(--ct-fg, #eee)", background: "var(--ct-bg, #14141c)" }}>
      <div style={{ fontSize: 11, textTransform: "uppercase", letterSpacing: "0.05em", color: "var(--ct-muted, #9aa)", marginBottom: 8 }}>Server</div>
      <div style={{ display: "flex", gap: 8, marginBottom: 18 }}>
        <input style={inputStyle} value={serverUrl} placeholder="https://vault.bitwarden.com"
          onChange={(e) => setServerUrl(e.target.value)}
          onBlur={() => void run(() => ct().invoke("setServerUrl", { url: serverUrl }), "save URL")} />
      </div>

      <div style={{ fontSize: 11, textTransform: "uppercase", letterSpacing: "0.05em", color: "var(--ct-muted, #9aa)", marginBottom: 8 }}>Connection</div>
      {!status ? (
        <span style={{ color: COLOR.muted, fontSize: 12.5 }}>Checking status…</span>
      ) : st === "unlocked" ? (
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
          <Badge tone="ok" label={`Unlocked${status.user ? ` · ${status.user}` : ""}`} />
          <button style={{ ...btnStyle, background: "rgba(255,255,255,0.08)" }} disabled={busy}
            onClick={() => void run(() => ct().invoke("signout"), "sign out")}>Sign out</button>
        </div>
      ) : st === "unavailable" ? (
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <Badge tone="danger" label="bw not found" />
          <span style={{ color: COLOR.muted, fontSize: 11.5 }}>{status.reason || "The Bitwarden CLI isn't available."}</span>
        </div>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <Badge tone={needsLogin ? "danger" : "warn"} label={needsLogin ? "Not signed in" : "Locked"} />
          <span style={{ color: COLOR.muted, fontSize: 11.5 }}>
            {needsLogin ? "Enter your Bitwarden email and master password." : "Enter your master password to unlock."}
          </span>
          {needsLogin && (
            <input style={inputStyle} type="email" autoComplete="username" placeholder="Bitwarden email"
              value={email} onChange={(e) => setEmail(e.target.value)} />
          )}
          <div style={{ display: "flex", gap: 8 }}>
            <input style={inputStyle} type="password" autoComplete="current-password" placeholder="Master password"
              value={master} onChange={(e) => setMaster(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Enter" && canSubmit && !busy) void run(() => ct().invoke("unlock", { masterPassword: master, email: email.trim() || undefined, twoFactorToken: twoFactor.trim() || undefined }), "unlock"); }} />
            <button style={btnStyle} disabled={busy || !canSubmit}
              onClick={() => void run(() => ct().invoke("unlock", { masterPassword: master, email: email.trim() || undefined, twoFactorToken: twoFactor.trim() || undefined }), "unlock")}>
              {busy ? "Unlocking…" : needsLogin ? "Sign in" : "Unlock"}
            </button>
          </div>
          {needsLogin && (
            <input style={inputStyle} inputMode="numeric" placeholder="2FA code (if enabled)"
              value={twoFactor} onChange={(e) => setTwoFactor(e.target.value)} />
          )}
          {busy && <span style={{ color: COLOR.muted, fontSize: 11 }}>Talking to the Bitwarden CLI — this can take 20–30s for self-hosted.</span>}
        </div>
      )}
      {error && <div style={{ color: COLOR.danger, fontSize: 12, marginTop: 10 }}>{error}</div>}
    </div>
  );
}

const el = document.getElementById("ct-root");
if (el) createRoot(el).render(<App />);
