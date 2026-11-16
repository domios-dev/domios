import { useId, useSyncExternalStore } from "react";
import { FormDialog } from "../shared/FormDialog";
import { Button } from "../shared/Button";
import { ConsentLayout } from "../shared/ConsentLayout";
import { decidePluginConsent, getPluginConsent, subscribePluginConsent } from "../../lib/pluginConsentStore";

export function PluginConsentDialog() {
  const request = useSyncExternalStore(subscribePluginConsent, getPluginConsent);
  const titleId = useId();
  const descriptionId = useId();
  if (!request) return null;
  return <FormDialog key={`${request.pluginId}:${request.permission}`} titleId={titleId} descriptionId={descriptionId}
    onCancel={() => decidePluginConsent(false, request)} onSubmit={(event) => { event.preventDefault(); decidePluginConsent(true, request); }}>
    <ConsentLayout titleId={titleId} descriptionId={descriptionId}
      title={<span data-plugin-consent>Allow {request.pluginName} access?</span>}
      description={`${request.pluginName} wants to ${request.label.toLowerCase()}. You can change this in plugin Permissions.`}
      actions={<>
        <Button type="button" variant="ghost" onClick={() => decidePluginConsent(false, request)}>Not now</Button>
        <Button type="submit" variant="primary">Allow</Button>
      </>} />
  </FormDialog>;
}
