import { TerminalSquare } from "lucide-react";
import { useShellProfiles } from "../../hooks/useShellProfiles";
import type { AppConfig } from "../../lib/types";
import { SettingsCard } from "./shared";

export function ShellProfilesCard({ config, onUpdate }: {
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
}) {
  const { profiles, error, loading, refresh } = useShellProfiles();
  const selected = config.terminal.default_shell ?? "";
  return (
    <SettingsCard icon={<TerminalSquare size={16} />} title="Shell">
      <label className="flex flex-col gap-1 text-xs">
        Default shell
        <select
          value={selected}
          onChange={(event) => onUpdate("terminal", { default_shell: event.target.value || null })}
          className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-[var(--ct-fg)]"
        >
          <option value="">Automatic</option>
          {selected && !profiles.some((profile) => profile.id === selected) && (
            <option value={selected}>{selected} (unavailable)</option>
          )}
          {profiles.map((profile) => <option key={profile.id} value={profile.id}>{profile.name}</option>)}
        </select>
      </label>
      <button type="button" disabled={loading} onClick={() => void refresh()} className="text-xs text-[var(--ct-accent)] disabled:opacity-50">
        Refresh shells
      </button>
      {error && <p role="alert" className="text-xs">{error}</p>}
    </SettingsCard>
  );
}
