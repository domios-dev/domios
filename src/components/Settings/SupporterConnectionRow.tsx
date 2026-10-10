import { useRef, useState, type ReactNode } from "react";
import { ExternalLink } from "lucide-react";
import { KebabGlyph } from "../shared/RowMenuButton";
import { DropdownMenu, type DropdownMenuItem } from "../shared/DropdownMenu";
import { Button } from "../shared/Button";
import { AboutLink, SettingsPill } from "./shared";
import { SupporterTokenField } from "./SupporterTokenField";
import type { SupporterControls, SupporterConnection } from "./useDecisionToken";

type RemovalSelection = { adapter: string | null; model: string | null } | null;

interface Props {
  row: SupporterConnection;
  controls: SupporterControls;
  stale: boolean;
  unavailableReason?: string;
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

export function SupporterConnectionRow({ row, controls, stale, unavailableReason, children, removed, unconfirmedRemoval }: Props) {
  const trigger = useRef<HTMLDivElement>(null);
  const removalPending = useRef(false);
  const [confirming, setConfirming] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [repairing, setRepairing] = useState(false);
  const [editingToken, setEditingToken] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
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
  const tokenUnavailable = row.token_mode === "required" && (!row.token.present || row.token.mode_ok === false);
  const tokenReason = tokenUnavailable ? (row.token.present ? "Token needs repair" : "Token required") : undefined;
  const unavailable =
    stale || Boolean(failure) || Boolean(unavailableReason) || tokenUnavailable || Boolean(controls.saved[row.adapter] && !row.active);
  const statusLabel = unavailable ? "Unavailable" : row.active ? "Active" : "Inactive";
  const cancel = () => {
    if (controls.busy || removing) return;
    setConfirming(false);
    trigger.current?.querySelector<HTMLButtonElement>("button")?.focus();
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
  const run = (action: () => void) => {
    trigger.current?.querySelector<HTMLButtonElement>("button")?.focus();
    action();
  };
  const menuItems: DropdownMenuItem[] = [
    ...(!row.active
      ? [{ label: "Make active", disabled, onClick: () => run(() => void controls.activate(row.adapter)) }]
      : []),
    ...(failure || (controls.saved[row.adapter] && !row.active)
      ? [{ label: "Retry activation", disabled, onClick: () => run(() => void controls.activate(row.adapter)) }]
      : []),
    ...(repairable && !repairing
      ? [{ label: "Repair credentials", disabled, onClick: () => run(() => setRepairing(true)) }]
      : []),
    ...(row.active && (row.token_mode === "required" || row.token_mode === "optional")
      ? [{
          label: row.token.present ? "Replace token" : "Add token",
          disabled,
          onClick: () => run(() => setEditingToken(true)),
        }]
      : []),
    {
      label: "Delete",
      danger: true,
      disabled,
      onClick: () => run(() => setConfirming(true)),
    },
  ];
  const failureReason = failure?.message || (stale ? "Last-known status; refresh to verify." : unavailableReason || tokenReason);
  const tokenSummary = row.token_mode === "none"
    ? "No token needed"
    : !row.token_mode
      ? "Token requirements unknown"
      : row.token.present
        ? "Token saved"
        : "No token stored";

  return (
    <section
      tabIndex={-1}
      data-adapter={row.adapter}
      aria-label={`${name} connection`}
      className="ct-row flex flex-col gap-1.5 px-2 py-2"
    >
      <div className="flex min-w-0 items-center gap-2">
        <span className="min-w-0 flex-1 truncate text-sm text-[var(--ct-fg)]">{name}</span>
        <SettingsPill tone={statusLabel === "Active" ? "green" : statusLabel === "Unavailable" ? "amber" : "muted"}>
          {statusLabel}
        </SettingsPill>
        <div ref={trigger} className="shrink-0">
          <DropdownMenu
            items={menuItems}
            menuLabel={`${name} connection actions`}
            onOpenChange={setMenuOpen}
            className="shrink-0"
            trigger={
              <Button
                type="button"
                variant="ghost"
                size="sm"
                aria-label={`Actions for ${name}`}
                aria-haspopup="menu"
                aria-expanded={menuOpen}
                className="!h-7 !w-7 !min-h-7 !p-0"
              >
                <KebabGlyph />
              </Button>
            }
          />
        </div>
      </div>
      {failureReason && (
        <p role="alert" className="truncate text-xs text-[var(--ct-muted-fg)]" title={failureReason}>
          {stale && !failure ? "Last-known status; refresh to verify." : failureReason}
        </p>
      )}
      {!row.active && (controls.saved[row.adapter] || row.token.present) && (
        <p role="status" className="text-xs text-[var(--ct-muted-fg)]">
          {[controls.saved[row.adapter] ? "Saved, not active" : null, row.token.present ? "Token saved, hidden" : null]
            .filter(Boolean)
            .join(" · ")}
        </p>
      )}
      {row.active && (
        <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-xs text-[var(--ct-muted-fg)]">
          <label className="sr-only" htmlFor={`supporter-model-${row.adapter}`}>Model for {name}</label>
          <select
            id={`supporter-model-${row.adapter}`}
            aria-label={`Model for ${name}`}
            value={row.model ?? ""}
            className="ct-field max-w-52 min-w-28"
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
          {(row.token_mode === "required" || row.token_mode === "optional") ? (
            <SupporterTokenField
              adapter={row.adapter}
              mode={row.token_mode}
              hasToken={row.token.present}
              busy={disabled}
              saveToken={saveToken}
              showTrigger={false}
              editing={editingToken}
              onEditingChange={setEditingToken}
            />
          ) : (
            <span>{tokenSummary}</span>
          )}
          {children}
        </div>
      )}
      {repairable && repairing && (
        <div aria-label={`${name} credential repair`} className="flex flex-wrap items-center gap-2 rounded border border-[var(--ct-border)] p-2">
          <p className="text-xs text-[var(--ct-muted-fg)]">Repair credentials for {name}. Saving will not activate this connection or change its model.</p>
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
              ? `${name} is the current Spinal Cord. Removing this connection${row.token.present ? " and its stored token" : ""} will stop the Spinal Cord; there is no automatic fallback.`
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
      {!row.active && (server || settings || help) && (
        <details className="text-xs text-[var(--ct-muted-fg)]">
          <summary className="w-fit cursor-pointer">Connection info</summary>
          <div className="flex flex-wrap items-center gap-3 pt-1">
            {server && <span>Server: {server}</span>}
            {settings && <AboutLink href={settings} label="Open adapter settings" Icon={ExternalLink} />}
            {help && <AboutLink href={help} label="Adapter help" Icon={ExternalLink} />}
          </div>
        </details>
      )}
    </section>
  );
}
