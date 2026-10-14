import { useId, useSyncExternalStore } from "react";
import { FormDialog } from "../shared/FormDialog";
import { Button } from "../shared/Button";
import { decidePluginConsent, getPluginConsent, subscribePluginConsent } from "../../lib/pluginConsentStore";

export function PluginConsentDialog() {
  const request = useSyncExternalStore(subscribePluginConsent, getPluginConsent);
  const titleId = useId();
  const descriptionId = useId();
  if (!request) return null;
  return <FormDialog key={`${request.pluginId}:${request.permission}`} titleId={titleId} descriptionId={descriptionId}
    onCancel={() => decidePluginConsent(false, request)} onSubmit={(event) => { event.preventDefault(); decidePluginConsent(true, request); }}>
    <h2 data-plugin-consent id={titleId} className="text-base font-semibold">Allow {request.pluginName} access?</h2>
    <p id={descriptionId} className="text-sm text-[var(--ct-muted)]">{request.pluginName} wants to {request.label.toLowerCase()}. You can change this in plugin Permissions.</p>
    <div className="flex justify-end gap-2">
      <Button type="button" variant="ghost" onClick={() => decidePluginConsent(false, request)}>Not now</Button>
      <Button type="submit" variant="primary">Allow</Button>
    </div>
  </FormDialog>;
}
