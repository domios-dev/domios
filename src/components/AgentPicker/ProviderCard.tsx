import { AgentIcon } from "../AgentIcon";
import { UI_BUTTON } from "../shared/design";
import type { ProviderDescriptor } from "../../lib/types";
import { isProviderInstalled } from "./preselectProvider";

interface ProviderCardProps {
  provider: ProviderDescriptor;
  selected: boolean;
  installed: boolean;
  disabled?: boolean;
  onSelect: () => void;
}

export function ProviderCard({
  provider,
  selected,
  installed,
  disabled,
  onSelect,
}: ProviderCardProps) {
  return (
    <button
      type="button"
      data-testid={`agent-picker-card-${provider.id}`}
      data-installed={installed ? "yes" : "no"}
      data-selected={selected ? "yes" : "no"}
      disabled={disabled}
      onClick={onSelect}
      className={[
        "flex flex-col items-center gap-2 rounded-xl border px-4 py-3 min-w-[7.5rem] transition-colors",
        selected
          ? "border-[var(--ct-accent)] bg-[var(--ct-accent)]/10 text-[var(--ct-fg)]"
          : "border-[var(--ct-border)] bg-[var(--ct-bg)]/60 text-[var(--ct-muted-fg)] hover:border-[var(--ct-accent)]/40 hover:text-[var(--ct-fg)]",
        disabled ? "opacity-50 cursor-not-allowed" : "cursor-pointer",
      ].join(" ")}
    >
      <AgentIcon providerId={provider.id} className="text-2xl" />
      <span className="text-xs font-medium">{provider.display_name}</span>
      <span
        className={[
          "text-[9px] px-1.5 py-0.5 rounded-full",
          installed
            ? "bg-[var(--ct-green)]/15 text-[var(--ct-green)]"
            : "bg-[var(--ct-border)]/50 text-[var(--ct-muted)]",
        ].join(" ")}
      >
        {installed ? "installed" : "not installed"}
      </span>
    </button>
  );
}

interface ProviderInstallPanelProps {
  provider: ProviderDescriptor;
  onRecheck: () => void;
  onInstall: () => void;
  busy?: boolean;
  checking?: boolean;
}

export function ProviderInstallPanel({
  provider,
  onRecheck,
  onInstall,
  busy,
  checking,
}: ProviderInstallPanelProps) {
  const installCommand = provider.install_command;
  return (
    <div
      data-testid={`agent-picker-install-${provider.id}`}
      className="rounded-xl border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4 space-y-2"
    >
      <div className="flex items-center gap-2">
        <AgentIcon providerId={provider.id} className="text-lg" />
        <div className="min-w-0">
          <div className="text-xs font-medium text-[var(--ct-fg)]">{provider.display_name}</div>
          {provider.cli_binary && (
            <div className="text-[10px] text-[var(--ct-muted-fg)] font-mono truncate">
              {provider.cli_binary}
            </div>
          )}
        </div>
      </div>
      {installCommand ? (
        <div className="text-[10px] text-[var(--ct-muted)] font-mono bg-[var(--ct-bg)] rounded px-2 py-1.5 overflow-x-auto">
          {installCommand}
        </div>
      ) : (
        <div className="text-[10px] text-[var(--ct-muted-fg)]">
          No install command for this platform — install manually, then re-check.
        </div>
      )}
      <div className="flex items-center justify-end gap-2">
        <button type="button" onClick={onRecheck} disabled={checking} className={UI_BUTTON.ghost}>
          {checking ? "Checking…" : "Re-check"}
        </button>
        {installCommand && (
          <button type="button" onClick={onInstall} disabled={busy} className={UI_BUTTON.accent}>
            {busy ? "Spawning…" : "Install"}
          </button>
        )}
      </div>
    </div>
  );
}

export function providerInstallSortKey(provider: ProviderDescriptor): number {
  return provider.id === "claude" ? 0 : 1;
}

export { isProviderInstalled };
