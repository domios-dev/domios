import type {ProviderDescriptor} from "../../lib/types";
import type {ExecutionLocation} from "../../lib/remoteExecution";
import {Button} from "../shared/Button";

export function ExecutionLocationPicker({provider, location, environmentId, onLocation, onEnvironment, onRefresh}: {
  provider: ProviderDescriptor | undefined; location: ExecutionLocation; environmentId: string;
  onLocation: (location: ExecutionLocation) => void; onEnvironment: (id: string) => void; onRefresh: () => void;
}) {
  if (!provider?.remote_task) return null;
  const capability = provider.remote_task;
  return <fieldset className="my-3 space-y-2">
    <legend>Execution location</legend>
    <label className="mr-3"><input type="radio" name="execution-location" value="local" checked={location === "local"} onChange={() => onLocation("local")} /> Local</label>
    <label><input type="radio" name="execution-location" value="cloud" checked={location === "cloud"} onChange={() => onLocation("cloud")} /> {capability.displayName}</label>
    {location === "cloud" && <div>
      {capability.availability.status === "unavailable" && <p role="status">{capability.availability.reason}</p>}
      {capability.environments.length > 0 ? <label>Environment<select aria-label="Cloud environment" value={environmentId} onChange={event => onEnvironment(event.target.value)}>
        <option value="">Choose an environment</option>
        {capability.environments.map(environment => <option key={environment.id} value={environment.id}>{environment.label}</option>)}
      </select></label> : <p role="status">No remote environments are available. Connect the account that owns your Cloud environments, then refresh providers.</p>}
      <Button variant="ghost" onClick={onRefresh}>Refresh providers</Button>
      <p>Remote tasks use your provider’s Cloud allowance. One task runs per tab.</p>
    </div>}
  </fieldset>;
}
