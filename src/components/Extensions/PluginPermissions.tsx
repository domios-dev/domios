import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useState } from "react";
import type { SensitivePermission } from "../../lib/pluginConsentModel";
import { setPluginPermission } from "../../lib/pluginConsent";
import { LiquidToggle } from "../shared/LiquidToggle";
import { GroupLabel } from "./shared";

export function PluginPermissions({ pluginId, permissions }: { pluginId: string; permissions?: SensitivePermission[] | null }) {
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  if (!permissions?.length) return null;
  async function change(key: string, allowed: boolean) {
    setBusy(key);
    setError(null);
    try { await setPluginPermission(pluginId, key, allowed); }
    catch (failure) { setError(String(failure)); }
    finally { setBusy(null); }
  }
  return (
    <section
      aria-label="Permissions"
      className="ct-card ct-card--static p-4"
    >
      <GroupLabel>Permissions</GroupLabel>
      <div className="divide-y divide-[var(--ct-border-subtle)]">
        {permissions.map((permission) => (
          <div key={permission.key} className="flex items-center justify-between gap-3 py-2 first:pt-0 last:pb-0">
            <div className="min-w-0">
              <div className="text-[12.5px] font-medium text-[var(--ct-fg)]">{permission.label}</div>
              <div className="text-[11px] text-[var(--ct-muted-fg)]">
                {permission.allowed ? "Allowed" : "Not allowed"}
              </div>
            </div>
            <LiquidToggle
              aria-label={permission.label}
              checked={permission.allowed}
              disabled={busy !== null}
              onCheckedChange={(allowed) => void change(permission.key, allowed)}
            />
          </div>
        ))}
      </div>
      {error && (
        <div className="mt-2">
          <InlineErrorNotice error={error} operation="changing plugin permissions" blocking />
        </div>
      )}
    </section>
  );
}
