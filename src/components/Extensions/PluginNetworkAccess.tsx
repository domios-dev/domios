import { useState } from "react";
import type { CatalogEntry } from "./pluginCatalog";
import { GroupLabel } from "./shared";
import { Button } from "../shared/Button";

function normalizeNetworkHost(value: string): string | null {
  const candidate = value.trim();
  if (!candidate || candidate !== value || /[\s*\/?#@%]|:\/\//.test(candidate)) return null;
  let host: string;
  let port = "";
  if (candidate.startsWith("[")) {
    const end = candidate.indexOf("]");
    if (end < 0) return null;
    host = candidate.slice(0, end + 1);
    const rest = candidate.slice(end + 1);
    if (rest && !rest.startsWith(":")) return null;
    port = rest.slice(1);
  } else {
    const separator = candidate.lastIndexOf(":");
    if (separator >= 0) {
      if (candidate.indexOf(":") !== separator) return null;
      host = candidate.slice(0, separator);
      port = candidate.slice(separator + 1);
    } else {
      host = candidate;
    }
  }
  if (candidate.endsWith(":") || (port && (!/^\d+$/.test(port) || Number(port) < 1 || Number(port) > 65535))) return null;
  try {
    const parsed = new URL(`http://${host}`);
    const hostname = parsed.hostname.toLowerCase().replace(/\.$/, "");
    if (!hostname || parsed.username || parsed.password || parsed.pathname !== "/") return null;
    if (/[\u0080-\uffff]/.test(host)) return null;
    if (!hostname.startsWith("[") && (
      hostname.length > 253
      || !hostname.split(".").every((label) =>
        label.length > 0
        && label.length <= 63
        && /^[a-z0-9]/.test(label)
        && /[a-z0-9]$/.test(label)
        && /^[a-z0-9-]+$/.test(label),
      )
    )) return null;
    const normalizedHost = host.startsWith("[")
      ? hostname
      : host.toLowerCase().replace(/\.$/, "");
    return port ? `${normalizedHost}:${Number(port)}` : normalizedHost;
  } catch {
    return null;
  }
}

function baseUrlHost(value: string): string | null {
  const authority = /^[a-z][a-z0-9+.-]*:\/\/([^/?#]*)/i.exec(value)?.[1];
  return authority ? normalizeNetworkHost(authority) : null;
}

function hostMatches(allowedValue: string, requestedValue: string): boolean {
  const allowed = normalizeNetworkHost(allowedValue);
  const requested = normalizeNetworkHost(requestedValue);
  if (!allowed || !requested) return false;
  const split = (authority: string) => {
    if (authority.startsWith("[")) {
      const end = authority.indexOf("]");
      const host = authority.slice(0, end + 1);
      const suffix = authority.slice(end + 1);
      return [host, suffix.slice(1)] as const;
    }
    const colon = authority.lastIndexOf(":");
    return colon < 0 ? [authority, ""] as const : [authority.slice(0, colon), authority.slice(colon + 1)] as const;
  };
  const [allowedHost, allowedPort] = split(allowed);
  const [requestedHost, requestedPort] = split(requested);
  const allowedHasPort = allowedPort.length > 0;
  const requestedHasPort = requestedPort.length > 0;
  return allowedHost === requestedHost && (!allowedHasPort || (requestedHasPort && allowedPort === requestedPort));
}

export function PluginNetworkAccess({
  entry,
  baseUrl,
  busy,
  onChange,
}: {
  entry: CatalogEntry;
  baseUrl: string;
  busy: boolean;
  onChange: (host: string, allow: boolean) => void;
}) {
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const allowed = entry.networkAllow ?? [];
  const effective = entry.effectiveNetworkAllow ?? [];
  const configuredHost = baseUrlHost(baseUrl);
  const missingBaseHost = configuredHost && !effective.some((host) => hostMatches(host, configuredHost))
    ? configuredHost
    : null;
  const normalizedDraft = normalizeNetworkHost(draft);

  const add = () => {
    if (!normalizedDraft) {
      setError("Enter an exact hostname or IP, with an optional port. Schemes and wildcards are not allowed.");
      return;
    }
    if (allowed.some((host) => hostMatches(host, normalizedDraft) && hostMatches(normalizedDraft, host))) {
      setError(`${normalizedDraft} is already allowed.`);
      return;
    }
    setError(null);
    onChange(normalizedDraft, true);
    setDraft("");
  };

  return (
    <details
      data-testid="extensions-allowed-hosts"
      className="ct-card ct-card--static px-4 py-3"
    >
      <summary className="cursor-pointer text-xs font-medium text-[var(--ct-muted-fg)] select-none hover:text-[var(--ct-fg)]">
        Network access (advanced)
      </summary>
      <div className="mt-3 space-y-3">
        <p className="m-0 text-xs leading-relaxed text-[var(--ct-muted-fg)]">
          Lets this plugin reach extra servers beyond the ones it declares.
        </p>
        <div>
          <GroupLabel className="mb-1.5 text-xs">Declared servers</GroupLabel>
          <ul aria-label="Declared servers" className="m-0 flex list-none flex-wrap gap-1.5 p-0 text-xs text-[var(--ct-fg)]">
            {(entry.declaredNetworkAllow ?? []).map((host) => (
              <li key={host}>
                <code className="ct-text-mono rounded bg-[color-mix(in_srgb,var(--ct-fg)_5%,transparent)] px-1.5 py-0.5 text-[11px]">
                  {host}
                </code>
              </li>
            ))}
            {!entry.declaredNetworkAllow?.length && (
              <li className="text-xs text-[var(--ct-muted-fg)]">No servers declared.</li>
            )}
          </ul>
        </div>
        {missingBaseHost && (
          <div role="status" className="flex flex-wrap items-center gap-2 text-xs text-[var(--ct-fg)]">
            <span>Configured base URL host {missingBaseHost} is not allowed.</span>
            <Button variant="accent" size="sm" aria-label={`Allow ${missingBaseHost}`} onClick={() => onChange(missingBaseHost, true)} disabled={busy}>
              Allow {missingBaseHost}
            </Button>
          </div>
        )}
        <ul aria-label="Current allowed hosts" className="m-0 flex list-none flex-col gap-1.5 p-0">
          {allowed.map((host) => (
            <li key={host} className="flex items-center justify-between gap-2 text-xs">
              <code className="ct-text-mono text-[var(--ct-fg)]">{host}</code>
              <Button variant="ghost" size="sm" aria-label={`Remove ${host}`} onClick={() => onChange(host, false)} disabled={busy}>
                Remove
              </Button>
            </li>
          ))}
          {allowed.length === 0 && <li className="text-xs text-[var(--ct-muted-fg)]">No user-granted hosts.</li>}
        </ul>
        <div className="flex gap-2">
          <label htmlFor={`allowed-host-${entry.id}`} className="sr-only">Host or host:port</label>
          <input
            id={`allowed-host-${entry.id}`}
            aria-label="Host or host:port"
            className="ct-input ct-input--compact ct-input--mono ct-text-mono min-w-0 flex-1"
            value={draft}
            onChange={(event) => { setDraft(event.currentTarget.value); setError(null); }}
            onKeyDown={(event) => { if (event.key === "Enter") { event.preventDefault(); add(); } }}
            placeholder="api.example.com:8443"
            autoComplete="off"
            disabled={busy}
          />
          <Button variant="accent" size="sm" aria-label="Allow host" onClick={add} disabled={busy || !draft.trim()}>
            Allow host
          </Button>
        </div>
        {error && (
          <p role="alert" aria-label="Allowed host validation error" className="m-0 text-xs text-[var(--ct-red)]">
            {error}
          </p>
        )}
      </div>
    </details>
  );
}
