import { catalogueSelection, modelIdentity, selectedModel } from "./model-policy.mjs";
import { REQUIRED_PROVIDERS } from "./oracle.mjs";
import { fixtureProviderId } from "./fixture-control.mjs";
import { profileSelection, profileIdentity, selectionDefault } from "./profile-policy.mjs";
import { excludedProviders, runs } from "./selection.mjs";

// A null selection means the full suite with the complete required roster.
export function modelEvidenceErrors(state, { verify = true, builds = {}, selection = null } = {}) {
  const errors = [];
  const ran = (id) => !selection || runs(selection, id);
  const excluded = excludedProviders(selection);
  const coverage = ran("J1") ? "J1" : ran("J4") ? "J4" : null;
  const require = (condition, reason) => { if (!condition) throw new Error(reason); };
  const check = (body) => { try { body(); } catch (error) { errors.push(`model policy: ${error.message}`); } };
  const policy = state?.model_policy;
  if (!policy) return ["model policy: persisted policy absent"];
  check(() => {
    require(!policy.refusal, `refusal recorded: ${policy.refusal}`);
    require(Array.isArray(policy.roster) && policy.roster.length > 0, "roster absent");
    require(new Set(policy.roster).size === policy.roster.length, "duplicate roster provider");
    require(REQUIRED_PROVIDERS.every((id) => excluded.includes(id) || policy.roster.includes(id)), "required provider roster narrowed");
    require(!excluded.some((id) => policy.roster.includes(id) || Object.hasOwn(policy.selections ?? {}, id)), "an excluded provider remains in the roster");
    require(Object.keys(policy.selections).length === policy.roster.length, "selections differ from roster");
  });
  if (!Array.isArray(policy.roster) || !policy.selections || typeof policy.selections !== "object") return errors;
  let controlId = null;
  if (ran("J3")) check(() => {
    const provider = fixtureProviderId(state.run);
    const fixture = state.j3;
    const controls = state.tabs?.filter(tab => tab.control_fixture) ?? [];
    require(controls.length === 1, "control fixture row absent or ambiguous");
    const tab = controls[0];
    require(fixture?.fixture_provider === provider && fixture.orchestrator_provider === provider,
      "control fixture provider differs from this run");
    require(tab.id === fixture.orchestrator && tab.journey === "J3" && tab.role === "fixture_orchestrator"
      && (tab.provider == null || tab.provider === provider), "control fixture tab identity differs");
    require(!policy.roster.includes(provider) && !Object.hasOwn(policy.selections, provider), "control fixture entered the paid roster");
    const runner = fixture.runner;
    require(runner?.tab_id === tab.id && runner.session_id === fixture.orchestrator_session
      && typeof runner.session_id === "string" && runner.session_id.length > 0
      && typeof runner.binding_nonce === "string" && runner.binding_nonce.length > 0
      && Number.isSafeInteger(runner.pid) && runner.pid > 0, "control fixture runner binding absent or changed");
    require(typeof runner.queue === "string" && runner.queue.replaceAll("\\", "/").endsWith(`/runtime/agent-pads/${tab.id}/cj-runner/${runner.binding_nonce}`),
      "control fixture queue is outside its recorded launch pad");
    if (verify) {
      const before = state.before_restart?.tabs?.find(before => before.id === tab.id);
      require(before?.owned === true && before.agent_type === provider && before.backing === "pty"
        && before.session === runner.session_id, "control fixture restart session or provider changed");
    }
    controlId = tab.id;
  });
  const catalogue = (entry, provider, selected) => {
    require(entry?.provider === provider && entry.selected === selected, `${provider} catalogue selection absent or changed`);
    if (entry.kind === "profile") {
      const observed = profileSelection({ id: provider, agent_runtime: { launchProfiles: { [entry.launch_profile]: {} } } }, selected, [entry.profile]);
      require(observed.launch_profile === entry.launch_profile && observed.model === entry.model, `${provider} profile selection changed`);
      return;
    }
    catalogueSelection({ id: provider, availability: "available", model_catalogue: entry.capability,
      available_models: [entry.model], catalogue_version: entry.catalogue_version }, selected, entry.refresh,
      entry.qualification ? { qualification: entry.qualification, phase: verify ? "verify" : "setup", run: state.run,
        build_sha: builds.verify, setup_build_sha: builds.setup, setup_live: policy.setup_live?.[provider] } : undefined);
  };
  for (const provider of policy.roster ?? []) check(() => {
    const selected = selectedModel(policy.selections, provider);
    catalogue(policy.catalogues?.[provider], provider, selected);
    if (policy.verify_catalogues?.[provider]) catalogue(policy.verify_catalogues[provider], provider, selected);
    for (const phase of verify ? ["setup", "verify"] : ["setup"]) {
      const configuration = policy.configurations?.[phase];
      require(configuration?.defaults?.[provider] === selectionDefault(policy.catalogues[provider]), `${phase} ${provider} scoped default absent or substituted`);
      require(configuration.general_provider === policy.general_provider
        && configuration.general_model === selectedModel(policy.selections, policy.general_provider), `${phase} General selection absent or substituted`);
    }
    if (coverage) require(state.tabs?.some((tab) => tab.journey === coverage && tab.provider === provider), `${provider} ${coverage} roster coverage absent`);
  });
  for (const launch of policy.launches ?? []) check(() => {
    const selected = selectedModel(policy.selections, launch.provider);
    if (launch.catalogue?.kind === "profile") require(launch.selection === selected
      && launch.launch_profile === launch.catalogue.launch_profile && launch.model === launch.catalogue.model, `${launch.path} profile or model substituted`);
    else require(launch.model === selected, `${launch.path} model substituted`);
    catalogue(launch.catalogue, launch.provider, selected);
    require(typeof launch.checked_at === "string" && launch.checked_at.length > 0, "launch catalogue observation time absent");
  });
  const paths = [...(ran("J1") ? ["J1-spawn"] : coverage ? ["J4-spawn"] : []),
    ...(ran("J2") ? ["J2-api-pair", "J2-ui-pair"] : []), ...(ran("J3") ? ["J3-dispatch"] : [])];
  for (const path of paths) check(() => {
    require(policy.launches?.some((launch) => launch.path === path), `${path} prelaunch evidence absent`);
  });
  const identity = (entry, tab) => {
    const selected = selectedModel(policy.selections, tab.provider ?? tab.agent_type);
    require(entry?.selected === selected, `tab ${tab.id} selected identity absent`);
    const provider = tab.provider ?? tab.agent_type;
    const record = { id: tab.id, agent_type: entry.provider, agent_requested_model: entry.requested,
      agent_provider_resolved_model: entry.resolved, agent_session_model: entry.actual, agent_session_id: entry.session };
    if (policy.catalogues?.[provider]?.kind === "profile") {
      profileIdentity({ ...record, agent_runtime_config: { launchProfile: entry.launch_profile } }, provider, policy.catalogues[provider]);
      return;
    }
    modelIdentity({ id: tab.id, agent_type: entry.provider, agent_requested_model: entry.requested,
      agent_provider_resolved_model: entry.resolved, agent_session_model: entry.actual,
      agent_session_id: entry.session }, tab.provider ?? tab.agent_type, selected);
  };
  for (const tab of state.tabs?.filter((tab) => tab.id !== controlId) ?? []) check(() => {
    identity(policy.identities?.[tab.id], tab);
    if (verify) require(state.before_restart?.tabs?.some((before) => before.id === tab.id), `tab ${tab.id} restart snapshot absent`);
  });
  if (verify) for (const tab of state.before_restart?.tabs ?? []) {
    if (tab.id === controlId) continue;
    check(() => {
      identity(tab.model_identity, tab);
      identity(policy.restored_identities?.[tab.id], tab);
      require(tab.model_identity.session === policy.restored_identities[tab.id].session, `tab ${tab.id} restored policy session changed`);
    });
  }
  return errors;
}
