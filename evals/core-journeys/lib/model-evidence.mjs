import { catalogueSelection, modelIdentity, selectedModel } from "./model-policy.mjs";
import { REQUIRED_PROVIDERS } from "./oracle.mjs";

export function modelEvidenceErrors(state, { verify = true } = {}) {
  const errors = [];
  const require = (condition, reason) => { if (!condition) throw new Error(reason); };
  const check = (body) => { try { body(); } catch (error) { errors.push(`model policy: ${error.message}`); } };
  const policy = state?.model_policy;
  if (!policy) return ["model policy: persisted policy absent"];
  check(() => {
    require(!policy.refusal, `refusal recorded: ${policy.refusal}`);
    require(Array.isArray(policy.roster) && policy.roster.length > 0, "roster absent");
    require(new Set(policy.roster).size === policy.roster.length, "duplicate roster provider");
    require(REQUIRED_PROVIDERS.every((id) => policy.roster.includes(id)), "required provider roster narrowed");
    require(Object.keys(policy.selections).length === policy.roster.length, "selections differ from roster");
  });
  if (!Array.isArray(policy.roster) || !policy.selections || typeof policy.selections !== "object") return errors;
  const catalogue = (entry, provider, selected) => {
    require(entry?.provider === provider && entry.selected === selected, `${provider} catalogue selection absent or changed`);
    catalogueSelection({ id: provider, availability: "available", model_catalogue: entry.capability,
      available_models: [entry.model], catalogue_version: entry.catalogue_version }, selected, entry.refresh);
  };
  for (const provider of policy.roster ?? []) check(() => {
    const selected = selectedModel(policy.selections, provider);
    catalogue(policy.catalogues?.[provider], provider, selected);
    for (const phase of verify ? ["setup", "verify"] : ["setup"]) {
      const configuration = policy.configurations?.[phase];
      require(configuration?.defaults?.[provider] === selected, `${phase} ${provider} scoped default absent or substituted`);
      require(configuration.general_provider === policy.general_provider
        && configuration.general_model === selectedModel(policy.selections, policy.general_provider), `${phase} General selection absent or substituted`);
    }
    require(state.tabs?.some((tab) => tab.journey === "J1" && tab.provider === provider), `${provider} J1 roster coverage absent`);
  });
  for (const launch of policy.launches ?? []) check(() => {
    require(launch.model === selectedModel(policy.selections, launch.provider), `${launch.path} model substituted`);
    catalogue(launch.catalogue, launch.provider, launch.model);
    require(typeof launch.checked_at === "string" && launch.checked_at.length > 0, "launch catalogue observation time absent");
  });
  for (const path of ["J1-spawn", "J2-api-pair", "J2-ui-pair", "J3-dispatch"]) check(() => {
    require(policy.launches?.some((launch) => launch.path === path), `${path} prelaunch evidence absent`);
  });
  const identity = (entry, tab) => {
    const selected = selectedModel(policy.selections, tab.provider ?? tab.agent_type);
    require(entry?.selected === selected, `tab ${tab.id} selected identity absent`);
    modelIdentity({ id: tab.id, agent_type: entry.provider, agent_requested_model: entry.requested,
      agent_provider_resolved_model: entry.resolved, agent_session_model: entry.actual,
      agent_session_id: entry.session }, tab.provider ?? tab.agent_type, selected);
  };
  for (const tab of state.tabs?.filter((tab) => !tab.shell) ?? []) check(() => {
    identity(policy.identities?.[tab.id], tab);
    if (verify) require(state.before_restart?.tabs?.some((before) => before.id === tab.id), `tab ${tab.id} restart snapshot absent`);
  });
  if (verify) for (const tab of state.before_restart?.tabs ?? []) {
    if (state.tabs?.some((fixture) => fixture.id === tab.id && fixture.shell)) continue;
    check(() => {
      identity(tab.model_identity, tab);
      identity(policy.restored_identities?.[tab.id], tab);
      require(tab.model_identity.session === policy.restored_identities[tab.id].session, `tab ${tab.id} restored policy session changed`);
    });
  }
  return errors;
}
