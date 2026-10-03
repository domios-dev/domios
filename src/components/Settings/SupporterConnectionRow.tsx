import { useRef, useState, type ReactNode } from "react";
import { ExternalLink } from "lucide-react";
import { Button } from "../shared/Button";
import { UI_INPUT } from "../shared/design";
import { AboutLink, SettingsPill } from "./shared";
import { SupporterTokenField } from "./SupporterTokenField";
import type { SupporterControls, SupporterConnection } from "./useDecisionToken";

type RemovalSelection = { adapter: string | null; model: string | null } | null;

interface Props {
  row: SupporterConnection;
  controls: SupporterControls;
  stale: boolean;
  children?: ReactNode;
  removed: (selection: RemovalSelection) => void;
  unconfirmedRemoval: () => void;
}

function safeLink(value?: string | null) {
  if (!value) return undefined;
  try {
    const url = new URL(value);
    return ["https:", "http:"].includes(url.protocol) && !url.username && !url.password ? url.href : undefined;
  } catch {
    return undefined;
  }
}

export function SupporterConnectionRow({ row, controls, stale, children, removed, unconfirmedRemoval }: Props) {
  const menu = useRef<HTMLDetailsElement>(null);
  const removalPending = useRef(false);
  const [confirming, setConfirming] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [repairing, setRepairing] = useState(false);
  const disabled = controls.busy || controls.statusFailed;
  const name = row.metadata?.display_name ?? row.adapter;
  const options = controls.catalog.find((adapter) => adapter.id === row.adapter)?.models ?? [];
  const failure = controls.errors[row.adapter] ?? row.error;
  const repairable =
    !row.active &&
    row.token_mode === "required" &&
    (!row.token.present ||
      row.token.mode_ok === false ||
      failure?.code === "authentication_failed" ||
      failure?.code === "no_token" ||
      failure?.code === "token_unusable");
  const help = safeLink(row.metadata?.help_url);
  const settings = safeLink(row.metadata?.settings_url);
  const address = safeLink(row.metadata?.server_address);
  const server = address ? new URL(address).origin : undefined;
  const cancel = () => {
    if (controls.busy || removing) return;
    setConfirming(false);
    menu.current?.querySelector("summary")?.focus();
  };
  const remove = async () => {
    if (removalPending.current || disabled) return;
    removalPending.current = true;
    setRemoving(true);
    try {
      const result = await controls.disconnect(row.adapter);
      if (result.confirmed) removed(result.selection);
      else if (result.needsReconcile) unconfirmedRemoval();
    } finally {
      removalPending.current = false;
      setRemoving(false);
    }
  };
  const saveToken = async (adapter: string, token: string) => {
    const saved = await controls.saveToken(adapter, token);
    if (saved && repairing) setRepairing(false);
    return saved;
  };
  const modelName = options.find((model) => model.id === row.model)?.display_name ?? row.model ?? "Model unavailable";
  return (
    <section
      tabIndex={-1}
      data-adapter={row.adapter}
      aria-label={`${name} connection`}
      className="rounded-md border border-[var(--ct-border)] p-3 flex flex-col gap-2"
    >
      <div className="flex items-center justify-between gap-2">
        <span className="text-sm text-[var(--ct-fg)]">{name}</span>
        <SettingsPill tone={row.active && !stale ? "green" : "muted"}>
          {row.active ? (stale ? "Last known current" : "Active") : "Inactive"}
        </SettingsPill>
        {!row.active && (
          <Button variant="ghost" size="sm" disabled={disabled} onClick={() => void controls.activate(row.adapter)}>
            Make active
          </Button>
        )}
        <details ref={menu} className="relative">
          <summary aria-label={`Actions for ${name}`} className="cursor-pointer">
            Actions
          </summary>
          <Button
            variant="ghost"
            size="sm"
            disabled={disabled}
            onClick={() => {
              if (menu.current) menu.current.open = false;
              setConfirming(true);
            }}
          >
            Remove connection
          </Button>
        </details>
      </div>
      <span className="text-[11px] text-[var(--ct-muted)]">
        {stale
          ? "Last-known connection status"
          : row.active
            ? "Selected connection"
            : "Configured connection · reachability not verified"}
      </span>
      {controls.saved[row.adapter] && !row.active && <span role="status">Saved, not active</span>}
      <div className="flex flex-col gap-1">
        {row.active ? (
          <>
            <label htmlFor={`supporter-model-${row.adapter}`}>Model</label>
            <select
              id={`supporter-model-${row.adapter}`}
              value={row.model ?? ""}
              className={UI_INPUT.fullCompact}
              disabled={disabled || controls.catalogFailed || !options.length}
              onChange={(event) => void controls.selectModel(row.adapter, event.target.value)}
            >
              {!options.some((model) => model.id === row.model) && (
                <option value={row.model ?? ""}>{row.model ?? "No models available"}</option>
              )}
              {options.map((model) => (
                <option key={model.id} value={model.id}>
                  {model.display_name}
                </option>
              ))}
            </select>
          </>
        ) : (
          <span>Model: {modelName}</span>
        )}
      </div>
      {stale ? (
        <span>
          {row.token_mode === "none"
            ? "Token status last known: No token needed"
            : !row.token_mode
              ? "Token status last known: requirements unknown"
              : `Token status last known: ${row.token.present ? "Token saved, hidden" : "No token stored"}`}
        </span>
      ) : row.active ? (
        row.token_mode === "none" ? (
          <span className="text-xs">No token needed</span>
        ) : row.token_mode ? (
          <SupporterTokenField
            adapter={row.adapter}
            mode={row.token_mode}
            hasToken={row.token.present}
            busy={disabled}
            saveToken={saveToken}
          />
        ) : (
          <span>Token requirements unknown</span>
        )
      ) : !row.token_mode ? (
        <span>Token requirements unknown</span>
      ) : row.token_mode === "none" ? (
        <span className="text-xs">No token needed</span>
      ) : (
        <span>{row.token.present ? "Token saved, hidden" : "No token stored"}</span>
      )}
      {repairable && !repairing && (
        <Button variant="ghost" size="sm" disabled={disabled} onClick={() => setRepairing(true)}>
          Repair credentials
        </Button>
      )}
      {repairable && repairing && (
        <div aria-label={`${name} credential repair`} className="rounded border border-[var(--ct-border)] p-2">
          <p>Repair credentials for {name}. Saving will not activate this connection or change its model.</p>
          <SupporterTokenField
            adapter={row.adapter}
            mode="required"
            hasToken={row.token.present}
            busy={disabled || removing}
            saveToken={saveToken}
          />
          <Button variant="ghost" size="sm" disabled={disabled || removing} onClick={() => setRepairing(false)}>
            Cancel repair
          </Button>
        </div>
      )}
      {failure && (
        <div role="alert" className="text-xs text-[var(--ct-red)]">
          <span>
            {failure.code === "authentication_failed"
              ? "Authentication failed"
              : failure.code === "no_token"
                ? "Token required"
                : failure.code === "token_unusable"
                  ? "Token needs repair"
                  : "Unavailable"}: {failure.message}
          </span>
          <Button variant="ghost" size="sm" disabled={disabled} onClick={() => void controls.activate(row.adapter)}>
            Retry activation
          </Button>
        </div>
      )}
      {controls.saved[row.adapter] && !row.active && !failure && (
        <Button disabled={disabled} size="sm" onClick={() => void controls.activate(row.adapter)}>
          Retry activation
        </Button>
      )}
      {confirming && (
        <div
          role="alertdialog"
          aria-label={`Remove ${name} connection`}
          onKeyDown={(event) => {
            if (event.code === "Escape") {
              event.preventDefault();
              event.stopPropagation();
              if (!controls.busy && !removing) cancel();
            }
          }}
          className="rounded border border-[var(--ct-red)] p-2 text-xs"
        >
          <p>
            {row.active
              ? `${name} is the current Supporter. Removing this connection${row.token.present ? " and its stored token" : ""} will stop the Supporter; there is no automatic fallback.`
              : `Remove ${name} connection${row.token.present ? " and its stored token" : ""}?`}
          </p>
          {removing && <p role="status">Removal request pending. Escape will not cancel the server request.</p>}
          <Button autoFocus variant="ghost" size="sm" disabled={controls.busy || removing} onClick={cancel}>
            Keep
          </Button>
          <Button variant="danger" size="sm" disabled={disabled || removing} onClick={() => void remove()}>
            Remove connection
          </Button>
        </div>
      )}
      {server && <span className="text-xs">Server: {server}</span>}
      <div className="flex gap-3 text-xs">
        {settings && <AboutLink href={settings} label="Open adapter settings" Icon={ExternalLink} />}
        {help && <AboutLink href={help} label="Adapter help" Icon={ExternalLink} />}
      </div>
      {children}
    </section>
  );
}
