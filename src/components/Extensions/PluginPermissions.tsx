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
  return <section aria-label="Permissions" style={{ marginBottom: 18 }}>
    <GroupLabel>Permissions</GroupLabel>
    {permissions.map((permission) => <div key={permission.key} className="flex items-center justify-between gap-3 py-2">
      <div><div className="text-sm">{permission.label}</div><div className="text-xs text-[var(--ct-muted)]">{permission.allowed ? "Allowed" : "Not allowed"}</div></div>
      <LiquidToggle aria-label={permission.label} checked={permission.allowed} disabled={busy !== null}
        onCheckedChange={(allowed) => void change(permission.key, allowed)} />
    </div>)}
    {error && <InlineErrorNotice error={error} operation="changing plugin permissions" blocking />}
  </section>;
}
