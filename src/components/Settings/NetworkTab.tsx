import { useEffect, useState } from "react";
import { invoke } from "../../lib/bridge";
import { buildWebAccessUrls, serveHint, type WebAccessNetworkInfo } from "../../lib/webAccess";

export function NetworkTab() {
  return (
    <div className="space-y-5">
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

/**
 * Web Access — everything a browser/phone needs to reach this CodeTerm:
 * the /web URL on each known address and the API token to paste at the
 * token gate. Tokens for OTHER peers come from their own Web Access panel.
 */
function WebAccessSection() {
  const [token, setToken] = useState<string | null>(null);
  const [show, setShow] = useState(false);
  const [copied, setCopied] = useState(false);
  const [hosts, setHosts] = useState<string[]>([]);
  const [usingFallback, setUsingFallback] = useState(false);
  const [port, setPort] = useState(7685);

  useEffect(() => {
    void (async () => {
      try {
        const cfg = await invoke<{ api?: { token?: string; port?: number } }>("get_config");
        setToken(cfg.api?.token ?? null);
        const fallbackPort = cfg.api?.port ?? 7685;
        const status = await invoke<WebAccessNetworkInfo>("get_network_info").catch(
          () => ({}) as WebAccessNetworkInfo,
        );
        const picked = buildWebAccessUrls(status, fallbackPort);
        setHosts(picked.urls);
        setUsingFallback(picked.usingFallback);
        setPort(status.api_port ?? fallbackPort);
      } catch {
        /* settings panel stays usable without this section's data */
      }
    })();
  }, []);

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
      {usingFallback && (
        <p data-testid="web-access-serve-hint" className="text-xs text-[var(--ct-muted)] leading-snug">
          {serveHint(port)}
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
