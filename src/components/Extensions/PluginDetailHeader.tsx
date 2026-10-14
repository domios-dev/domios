import { useOptionalUpdatePanel } from "../../hooks/useUpdatePanel";
import { useEffect, useId, useRef, useState } from "react";
import { Trash2 } from "lucide-react";
import { KebabGlyph } from "../shared/RowMenuButton";
import { Button } from "../shared/Button";
import { Chip } from "../shared/Chip";
import { DropdownMenu, type DropdownMenuItem } from "../shared/DropdownMenu";
import { canUninstall, catalogKindLabel, primaryAction, type CatalogEntry } from "./pluginCatalog";
import { PluginIcon } from "./PluginIcon";
import { iconOf } from "./shared";
import { ConfigureWithAiButton } from "./ConfigureWithAiButton";
import { PluginActivityInline } from "./PluginActivityInline";
import { resolvePluginActivityDisplay, type PluginActivityDisplayMap } from "../../lib/usePluginActivity";
import type { PluginActivity } from "../../lib/types";

export type HeaderActionKind = "install" | "update" | "remove";
export type HeaderActionOptions = { removeData?: boolean };

export function PluginDetailHeader({
  entry,
  busy,
  onAction,
  activity,
  activityDisplayMap,
}: {
  entry: CatalogEntry;
  busy: string | null | undefined;
  onAction: (kind: HeaderActionKind, entry: CatalogEntry, options?: HeaderActionOptions) => void;
  activity?: PluginActivity;
  activityDisplayMap?: PluginActivityDisplayMap;
}) {
  const [confirmingRemove, setConfirmingRemove] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const updates = useOptionalUpdatePanel();
  const updateAvailable = updates ? updates.pluginHasUpdate(entry.id) : entry.updateAvailable;
  const action = primaryAction({ ...entry, updateAvailable });
  const anyBusy = busy?.endsWith(`:${entry.id}`) ?? false;
  const isBusy = (kind: HeaderActionKind) => busy === `${kind}:${entry.id}`;
  const removable = canUninstall(entry);
  const hasAiConfigure = entry.installed && !entry.pendingReload && !!entry.configHelp;
  const metadata = pluginMetadata(entry);

  const lifecycleKind: "install" | "update" | null = action.kind === "install" || action.kind === "update" ? action.kind : null;
  const lifecycle = lifecycleKind ? { ...action, kind: lifecycleKind } : null;
  const overflowItems: DropdownMenuItem[] = removable
    ? [{
        label: "Uninstall…",
        icon: <Trash2 size={13} aria-hidden="true" />,
        danger: true,
        disabled: anyBusy,
        onClick: () => setConfirmingRemove(true),
        "data-testid": "extensions-plugin-uninstall",
      }]
    : [];

  return (
    <>
      <header
        data-testid="extensions-plugin-header"
        className="mb-4 flex min-w-0 flex-wrap items-center gap-3"
      >
        <div className="flex h-11 w-11 flex-none items-center justify-center rounded-[10px] border border-[var(--ct-border-subtle)] bg-[var(--ct-bg-elev)]">
          <PluginIcon plugin={iconOf(entry)} size={22} />
        </div>
        <div className="min-w-0 flex-[1_1_140px]">
          <div className="flex min-w-0 items-center gap-2">
            <h2
              data-testid="extensions-plugin-name"
              title={entry.displayName}
              className="m-0 min-w-0 truncate text-[17px] font-[640] text-[var(--ct-fg)]"
            >
              {entry.displayName}
            </h2>
            {updateAvailable && <Chip variant="accent">Update</Chip>}
            {action.kind === "none" && (
              <span title={action.reason} className="flex-none" data-testid="extensions-plugin-origin">
                <Chip variant="muted">{action.label}</Chip>
              </span>
            )}
          </div>
          <div
            data-testid="extensions-plugin-metadata"
            title={`${entry.id} · ${metadata}`}
            className="mt-0.5 truncate text-[11px] text-[var(--ct-muted)]"
          >
            {metadata}
          </div>
          {activityDisplayMap && (
            <PluginActivityInline
              activity={activity}
              display={resolvePluginActivityDisplay(entry.id, activityDisplayMap)}
            />
          )}
        </div>
        <div className="ml-auto flex max-w-full flex-none items-center gap-1.5">
          {lifecycle && (
            <Button
              data-testid={`extensions-plugin-${lifecycle.kind}`}
              data-plugin-id={entry.id}
              variant={hasAiConfigure ? "accent" : "primary"}
              size="sm"
              onClick={() => onAction(lifecycle.kind, entry)}
              disabled={lifecycle.disabled || anyBusy}
              loading={isBusy(lifecycle.kind)}
              title={lifecycle.reason}
            >
              {lifecycle.label}
            </Button>
          )}
          <ConfigureWithAiButton entry={entry} />
          {removable && (
            <DropdownMenu
              items={overflowItems}
              menuLabel={`${entry.displayName} actions`}
              menuTestId="extensions-plugin-actions-menu"
              onOpenChange={setMenuOpen}
              trigger={
                <Button
                  variant="icon"
                  data-testid="extensions-plugin-actions"
                  data-plugin-id={entry.id}
                  aria-label={`More actions for ${entry.displayName}`}
                  aria-haspopup="menu"
                  aria-expanded={menuOpen}
                  title="More actions"
                  loading={isBusy("remove")}
                >
                  <KebabGlyph />
                </Button>
              }
            />
          )}
        </div>
      </header>
      {confirmingRemove && (
        <UninstallConfirm
          entry={entry}
          disabled={anyBusy}
          onCancel={() => setConfirmingRemove(false)}
          onConfirm={(removeData) => {
            setConfirmingRemove(false);
            onAction("remove", entry, { removeData });
          }}
        />
      )}
    </>
  );
}

function UninstallConfirm({
  entry,
  disabled,
  onCancel,
  onConfirm,
}: {
  entry: CatalogEntry;
  disabled: boolean;
  onCancel: () => void;
  onConfirm: (removeData: boolean) => void;
}) {
  const [removeData, setRemoveData] = useState(true);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const titleId = useId();
  useEffect(() => cancelRef.current?.focus(), []);

  return (
    <div
      role="alertdialog"
      aria-labelledby={titleId}
      data-testid="extensions-uninstall-confirm"
      data-plugin-id={entry.id}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.stopPropagation();
          onCancel();
        }
      }}
      className="mb-4 rounded-[10px] border border-[color-mix(in_srgb,var(--ct-red)_36%,transparent)] bg-[color-mix(in_srgb,var(--ct-red)_8%,transparent)] p-3"
    >
      <div id={titleId} className="mb-2 text-[12.5px] font-semibold text-[var(--ct-fg)]">
        Uninstall {entry.displayName}?
      </div>
      <label className="mb-3 flex items-center gap-2 text-xs text-[var(--ct-fg)]">
        <input
          type="checkbox"
          data-testid="extensions-uninstall-remove-data"
          checked={removeData}
          onChange={(event) => setRemoveData(event.currentTarget.checked)}
        />
        Also remove downloaded data
      </label>
      <div className="flex gap-2">
        <Button
          variant="danger"
          size="sm"
          data-testid="extensions-uninstall-confirm-submit"
          onClick={() => onConfirm(removeData)}
          disabled={disabled}
        >
          Uninstall
        </Button>
        <button
          ref={cancelRef}
          type="button"
          data-testid="extensions-uninstall-confirm-cancel"
          className="ct-btn ct-btn-ghost ct-btn-sm ct-liquid-interactive"
          onClick={onCancel}
          disabled={disabled}
        >
          Cancel
        </button>
      </div>
    </div>
  );
}

export function pluginMetadata(entry: CatalogEntry): string {
  const parts = [catalogKindLabel(entry)];
  if (entry.version) {
    parts.push(entry.updateAvailable && entry.remoteVersion ? `v${entry.version} → v${entry.remoteVersion}` : `v${entry.version}`);
  }
  if (entry.dev) parts.push("workspace source");
  else if (entry.channel) parts.push(channelLabel(entry.channel));
  return parts.join(" · ");
}

function channelLabel(channel: string): string {
  if (channel === "codeterm-core") return "Core";
  if (channel === "codeterm-plugins") return "Domios Plugins";
  return channel;
}
