import { useEffect, useState } from "react";
import { invoke } from "../../lib/bridge";
import {
  buildWebAccessUrls,
  planServeSetup,
  serveHint,
  type ServeSetupPlan,
  type ServeStatus,
  type WebAccessNetworkInfo,
} from "../../lib/webAccess";

interface ServeResult {
  ok: boolean;
  code: string;
  https_url?: string | null;
  used_fallback?: boolean;
  warning?: string | null;
}

export function NetworkTab() {
  return (
    <div className="space-y-5">
      <RestApiListenerStatus />
      <WebAccessSection />
      <Collapsible title="API Endpoints Reference">
        <div className="space-y-3">
          <EndpointGroup title="Workspaces" endpoints={[
            ["GET", "/api/workspaces", "List all"],
            ["POST", "/api/workspaces", "Create {name}"],
            ["POST", "/api/workspaces/:id/focus", "Switch"],
            ["POST", "/api/workspaces/:id/rename", "Rename {name}"],
            ["POST", "/api/workspaces/:id/close", "Delete"],
            ["POST", "/api/workspaces/:id/tabs", "Create tab"],
            ["POST", "/api/workspaces/:id/tabs/:tid/move", "Move tab"],
          ]} />
          <EndpointGroup title="Panes" endpoints={[
            ["GET", "/api/panes", "List all"],
            ["POST", "/api/panes/:id/focus", "Focus"],
            ["POST", "/api/panes/:id/close", "Close"],
            ["POST", "/api/panes/:id/send", "Send text (fire & forget)"],
            ["POST", "/api/panes/:id/exec", "Send + wait {command, timeout_ms}"],
            ["POST", "/api/panes/:id/rename", "Rename {title}"],
            ["POST", "/api/panes/:id/resize", "Resize {cols, rows}"],
            ["GET", "/api/panes/:id/output", "Get output {?offset=N}"],
            ["GET", "/api/panes/:id/output/stream", "SSE real-time stream"],
          ]} />
          <EndpointGroup title="Integration" endpoints={[
            ["POST", "/api/notifications", "Create {title, body, level}"],
            ["GET", "/api/network", "Connection info + IPs"],
            ["GET", "/api/serve-file", "Serve local file {?path=}"],
          ]} />
        </div>
      </Collapsible>
    </div>
  );
}

interface RestApiStatus {
  generation: number;
  phase: "starting" | "listening" | "failed";
  message?: string | null;
}

function RestApiListenerStatus() {
  const [status, setStatus] = useState<RestApiStatus | null>(null);
  const [restarting, setRestarting] = useState(false);

  async function refresh() {
    setStatus(await invoke<RestApiStatus>("get_rest_api_status"));
  }

  useEffect(() => { void refresh().catch(() => setStatus(null)); }, []);

  async function restart() {
    setRestarting(true);
    try {
      setStatus(await invoke<RestApiStatus>("restart_rest_api"));
    } catch (error) {
      setStatus((current) => current ? { ...current, message: String(error) } : current);
    } finally {
      setRestarting(false);
    }
  }

  if (!status || status.phase === "listening") return null;
  return (
    <div role="alert" className="rounded border border-amber-500/40 bg-amber-500/10 p-3 text-sm">
      <div className="font-medium">REST API listener {status.phase}</div>
      {status.message && <div className="mt-1 text-muted-foreground">{status.message}</div>}
      <button className="mt-2 rounded bg-amber-600 px-3 py-1 text-white disabled:opacity-60" disabled={restarting} onClick={() => void restart()}>
        {restarting ? "Restarting…" : "Restart REST API"}
      </button>
    </div>
  );
}

/**
 * Web Access — everything a browser/phone needs to reach this CodeTerm:
 * the browser URL on each known address and the API token to paste at the
 * token gate. Tokens for OTHER peers come from their own Web Access panel.
 */
function WebAccessSection() {
  const [token, setToken] = useState<string | null>(null);
  const [show, setShow] = useState(false);
  const [copied, setCopied] = useState(false);
  const [hosts, setHosts] = useState<string[]>([]);
  const [usingFallback, setUsingFallback] = useState(false);
  const [port, setPort] = useState(7685);
  const [serveStatus, setServeStatus] = useState<ServeStatus | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [resultMsg, setResultMsg] = useState<string | null>(null);

  async function refresh(fallbackPort: number) {
    const status = await invoke<WebAccessNetworkInfo>("get_network_info").catch(
      () => ({}) as WebAccessNetworkInfo,
    );
    const picked = buildWebAccessUrls(status, fallbackPort);
    setHosts(picked.urls);
    setUsingFallback(picked.usingFallback);
    setPort(status.api_port ?? fallbackPort);
    const serve = await invoke<ServeStatus>("get_serve_status").catch(() => null);
    setServeStatus(serve);
  }

  useEffect(() => {
    void (async () => {
      try {
        const cfg = await invoke<{ api?: { token?: string; port?: number } }>("get_config");
        setToken(cfg.api?.token ?? null);
        await refresh(cfg.api?.port ?? 7685);
      } catch {
        /* settings panel stays usable without this section's data */
      }
    })();
  }, []);

  const plan: ServeSetupPlan | null = serveStatus
    ? planServeSetup(serveStatus, port, serveStatus.platform ?? "")
    : null;

  // The exact-command preview only flips to a real mutation on the SECOND click:
  async function runEnable() {
    setBusy(true);
    setResultMsg(null);
    try {
      const res = await invoke<ServeResult>("enable_web_access", {});
      setConfirming(false);
      if (res.ok) {
        setResultMsg(
          res.warning ?? `Web Access enabled${res.https_url ? ` — ${res.https_url}/` : ""}`,
        );
      } else {
        setResultMsg(`Could not enable Web Access (${res.code}).`);
      }
      await refresh(port);
    } catch (e) {
      setResultMsg(`Enable failed: ${String(e)}`);
    } finally {
      setBusy(false);
    }
  }

  async function runDisable() {
    setBusy(true);
    setResultMsg(null);
    try {
      const res = await invoke<ServeResult>("disable_web_access", {});
      setResultMsg(res.ok ? "Web Access disabled." : `Could not disable (${res.code}).`);
      await refresh(port);
    } catch (e) {
      setResultMsg(`Disable failed: ${String(e)}`);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="rounded border border-[var(--ct-border)] p-4 space-y-3">
      <div className="text-sm font-medium text-[var(--ct-fg)]">Web Access</div>
      <p className="text-xs text-[var(--ct-muted)]">
        Open CodeTerm from a browser or phone. Paste the token below at the
        connection screen — it is this machine's API token, treat it like a
        password.
      </p>
      <div className="space-y-1">
        {hosts.map((h, i) => (
          <div
            key={h}
            className={`text-xs font-mono ${i === 0 ? "text-[var(--ct-fg)] font-medium" : "text-[var(--ct-muted-fg)]"}`}
          >
            {h}
          </div>
        ))}
      </div>
      {usingFallback && plan && (
        <WebAccessSetup
          plan={plan}
          port={port}
          confirming={confirming}
          busy={busy}
          onStart={() => {
            setResultMsg(null);
            setConfirming(true);
          }}
          onCancel={() => setConfirming(false)}
          onConfirm={() => void runEnable()}
        />
      )}
      {plan?.state === "active" && (
        <div
          data-testid="web-access-active"
          className="text-xs text-[var(--ct-muted)] leading-snug flex items-center justify-between gap-2"
        >
          <span className="text-[var(--ct-green)]">
            Web Access is active{serveStatus?.https_url ? ` — ${serveStatus.https_url}/` : ""}.
          </span>
          <button
            onClick={() => void runDisable()}
            disabled={busy}
            className="shrink-0 text-xs px-2 py-1 rounded border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] disabled:opacity-40"
          >
            Disable
          </button>
        </div>
      )}
      {resultMsg && (
        <p data-testid="web-access-result" className="text-xs text-[var(--ct-muted)] leading-snug">
          {resultMsg}
        </p>
      )}
      <div className="flex items-center gap-2">
        <code
          data-testid="web-access-token"
          className="flex-1 text-xs font-mono px-2 py-1.5 rounded bg-[var(--ct-bg)] border border-[var(--ct-border)] truncate"
        >
          {token ? (show ? token : "•".repeat(Math.min(token.length, 32))) : "no API token configured"}
        </code>
        <button
          onClick={() => setShow((v) => !v)}
          className="text-xs px-2 py-1.5 rounded border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]"
        >
          {show ? "Hide" : "Show"}
        </button>
        <button
          onClick={() => {
            if (!token) return;
            void navigator.clipboard.writeText(token).then(() => {
              setCopied(true);
              setTimeout(() => setCopied(false), 1500);
            });
          }}
          disabled={!token}
          className="text-xs px-2 py-1.5 rounded border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] disabled:opacity-40"
        >
          {copied ? "Copied ✓" : "Copy"}
        </button>
      </div>
    </div>
  );
}

/**
 * The consent-gated "Enable Web Access" affordance, shown only while falling
 * back to a raw IP+port URL. Enabling runs a Tailscale Serve mutation, so it is
 * strictly two-step: the first click reveals the EXACT commands that will run;
 * only "Confirm & Enable" actually invokes the backend.
 */
function WebAccessSetup({
  plan,
  port,
  confirming,
  busy,
  onStart,
  onCancel,
  onConfirm,
}: {
  plan: ServeSetupPlan;
  port: number;
  confirming: boolean;
  busy: boolean;
  onStart: () => void;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  if (plan.state === "not-installed") {
    return (
      <p data-testid="web-access-not-installed" className="text-xs text-[var(--ct-muted)] leading-snug">
        Install Tailscale to get a clean, phone-friendly HTTPS link. Without it,
        use one of the addresses above.
      </p>
    );
  }
  if (plan.state === "port-collision") {
    return (
      <p data-testid="web-access-port-collision" className="text-xs text-[var(--ct-yellow)] leading-snug">
        Can't auto-enable: the public HTTPS port ({plan.httpsPort}) would collide
        with a CodeTerm API port. Configure Tailscale Serve manually.
      </p>
    );
  }
  if (plan.state !== "can-enable") return null;

  if (!confirming) {
    return (
      <div className="space-y-2">
        <p data-testid="web-access-serve-hint" className="text-xs text-[var(--ct-muted)] leading-snug">
          {serveHint(port)}
        </p>
        <button
          data-testid="web-access-enable"
          onClick={onStart}
          className="text-xs px-2.5 py-1.5 rounded border border-[var(--ct-border)] text-[var(--ct-fg)] hover:bg-[var(--ct-hover)]"
        >
          Enable Web Access
        </button>
      </div>
    );
  }

  return (
    <div
      data-testid="web-access-confirm"
      className="space-y-2 rounded border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-2.5"
    >
      <p className="text-xs text-[var(--ct-fg)] leading-snug">
        This runs the following on your machine:
      </p>
      <pre className="text-[11px] font-mono text-[var(--ct-muted-fg)] whitespace-pre-wrap break-all">
        {plan.commands.join("\n")}
      </pre>
      {plan.resultUrl && (
        <p className="text-xs text-[var(--ct-muted)] leading-snug">
          Result: <span className="font-mono">{plan.resultUrl}</span>
        </p>
      )}
      <div className="flex items-center gap-2">
        <button
          data-testid="web-access-confirm-enable"
          onClick={onConfirm}
          disabled={busy}
          className="text-xs px-2.5 py-1.5 rounded border border-[var(--ct-accent)] text-[var(--ct-accent)] hover:bg-[var(--ct-hover)] disabled:opacity-40"
        >
          {busy ? "Enabling…" : "Confirm & Enable"}
        </button>
        <button
          onClick={onCancel}
          disabled={busy}
          className="text-xs px-2.5 py-1.5 rounded border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] disabled:opacity-40"
        >
          Cancel
        </button>
      </div>
    </div>
  );
}

function Collapsible({ title, children }: { title: string; children: React.ReactNode }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="border border-[var(--ct-border)] rounded-xl overflow-hidden bg-[var(--ct-bg)]/40">
      <button
        onClick={() => setOpen((v) => !v)}
        className="flex items-center justify-between w-full px-4 py-2 text-xs font-medium text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] transition-colors"
      >
        <span>{title}</span>
        <span className="text-[10px]">{open ? "▴" : "▾"}</span>
      </button>
      {open && (
        <div className="px-4 py-3 border-t border-[var(--ct-border)] bg-[var(--ct-bg)]/30">
          {children}
        </div>
      )}
    </div>
  );
}

function EndpointGroup({ title, endpoints }: { title: string; endpoints: [string, string, string][] }) {
  const methodColors: Record<string, string> = {
    GET: "text-[var(--ct-green)]",
    POST: "text-[var(--ct-accent)]",
    PUT: "text-[var(--ct-yellow)]",
    DELETE: "text-[var(--ct-red)]",
  };
  return (
    <div>
      <div className="text-[10px] font-semibold uppercase tracking-wider text-[var(--ct-muted)] mb-1">{title}</div>
      <div className="space-y-0.5">
        {endpoints.map(([method, path, desc], i) => (
          <div key={i} className="flex items-start gap-2 text-[10px] font-mono leading-relaxed">
            <span className={`shrink-0 w-8 font-bold ${methodColors[method] || "text-[var(--ct-muted-fg)]"}`}>{method}</span>
            <span className="text-[var(--ct-fg)] shrink-0">{path}</span>
            <span className="text-[var(--ct-muted)] truncate">{desc}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
