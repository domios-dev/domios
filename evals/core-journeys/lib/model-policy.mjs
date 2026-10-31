import { installed, selectProviders } from "./oracle.mjs";

const refuse = (reason) => { throw new Error(`model policy refused: ${reason}`); };
const nonempty = (value) => typeof value === "string" && value.trim() === value && value.length > 0;
const same = (left, right) => JSON.stringify([...left].sort()) === JSON.stringify([...right].sort());

export function modelSelections(entries) {
  const selections = Object.create(null);
  for (const entry of entries) {
    const separator = typeof entry === "string" ? entry.indexOf("=") : -1;
    if (typeof entry !== "string") refuse("--model requires PROVIDER=ADVERTISED_ID");
    const provider = entry.slice(0, separator), model = entry.slice(separator + 1);
    if (separator < 1 || !nonempty(provider) || !nonempty(model) || model.includes("=")) refuse("--model requires PROVIDER=ADVERTISED_ID");
    if (Object.hasOwn(selections, provider)) refuse(`duplicate selection for ${provider}`);
    selections[provider] = model;
  }
  return selections;
}

export function selectedModel(selections, provider) {
  if (!Object.hasOwn(selections ?? {}, provider) || !nonempty(selections[provider])) refuse(`missing explicit selection for ${provider}`);
  return selections[provider];
}

export function catalogueSelection(descriptor, selection, refresh) {
  if (!installed(descriptor)) refuse(`${descriptor?.id ?? "unknown"} unavailable`);
  const capability = descriptor.model_catalogue;
  if (!capability || !["authoritative", "free_form"].includes(capability.authority)
    || typeof capability.liveDiscoveryRequired !== "boolean") refuse(`${descriptor.id} catalogue capability unknown`);
  if (refresh?.provider_id !== descriptor.id || typeof refresh.refreshed !== "boolean"
    || typeof refresh.discovery_required !== "boolean" || refresh.discovery_required !== capability.liveDiscoveryRequired
    || (refresh.reason && !(capability.liveDiscoveryRequired === false && refresh.refreshed === false
      && refresh.reason === "the provider reported no live model list")) || (capability.liveDiscoveryRequired && !refresh.refreshed)) refuse(`${descriptor.id} catalogue refresh unavailable`);
  if (!Number.isSafeInteger(descriptor.catalogue_version) || !Number.isSafeInteger(refresh.version)) refuse(`${descriptor.id} catalogue version unknown`);
  if (!Array.isArray(descriptor.available_models) || !descriptor.available_models.length) refuse(`${descriptor.id} catalogue absent`);
  const matches = descriptor.available_models.filter((model) => model.id === selection);
  if (matches.length !== 1) refuse(`${descriptor.id} unknown or ambiguous advertised model ${selection}`);
  const model = matches[0];
  if (model.available === false || (model.availability != null && model.availability !== "available")) refuse(`${descriptor.id}/${selection} capability unavailable or unknown`);
  return { provider: descriptor.id, selected: selection, catalogue_version: descriptor.catalogue_version,
    capability, model, refresh };
}

export function modelIdentity(record, provider, selected, { first = false, previous = null } = {}) {
  if (!record || record.agent_type !== provider) refuse(`tab provider is not ${provider}`);
  if (record.model_fallback || record.model_identity_mismatch) refuse(`tab ${record.id} substitution reported`);
  const requested = record.agent_requested_model;
  const resolved = record.agent_provider_resolved_model;
  const actual = record.agent_session_model;
  if (requested !== selected || resolved !== selected) refuse(`tab ${record.id} requested/resolved identity unknown or mismatched`);
  if (actual != null && actual !== selected) refuse(`tab ${record.id} actual model mismatched`);
  if (!first && !nonempty(record.agent_session_id)) refuse(`tab ${record.id} own session unknown`);
  if (!first && actual !== selected) refuse(`tab ${record.id} actual model unknown; no further paid turn permitted`);
  if (previous && (!previous.session || record.agent_session_id !== previous.session)) refuse(`tab ${record.id} policy session changed`);
  return { provider, selected, requested, resolved, actual: actual ?? null, session: record.agent_session_id ?? null };
}

export function generalSelection(config, provider, selected) {
  const general = config.general_agent;
  if (general?.model && general.model !== selected) refuse("conflicting General model override");
  if (general?.first_run_complete && general.agent_provider !== provider) refuse("conflicting General provider");
}

export async function freshCatalogue(api, provider, selected, expectedRoster) {
  const refresh = await api("/api/agents/providers/refresh", { method: "POST", body: { provider_id: provider } });
  const descriptors = await api("/api/agents/providers");
  if (expectedRoster && !same(descriptors.filter(installed).map((descriptor) => descriptor.id), expectedRoster)) refuse("installed provider roster changed");
  return catalogueSelection(descriptors.find((descriptor) => descriptor.id === provider), selected, refresh);
}

export async function preparePolicy(ctx, { readRecord, configure = false, required } = {}) {
  const selections = Object.freeze({ ...ctx.options.models });
  const descriptors = await ctx.api("/api/agents/providers");
  const roster = selectProviders(descriptors, { required });
  if (roster.missing.length) refuse(`required providers missing: ${roster.missing.map((entry) => entry.id).join(",")}`);
  const ids = roster.run.map((provider) => provider.id);
  if (ctx.options.providers && !same(ctx.options.providers, ids)) refuse("provider roster narrowing is forbidden");
  if (!same(Object.keys(selections), ids)) refuse("explicit selections must cover the complete installed provider roster");
  const previous = ctx.state.model_policy;
  if (previous?.refusal) refuse(`persisted refusal: ${previous.refusal}`);
  if (previous && (!same(previous.roster, ids) || !same(Object.entries(previous.selections).map((entry) => JSON.stringify(entry)), Object.entries(selections).map((entry) => JSON.stringify(entry))))) refuse("persisted restart policy differs from explicit selections or roster");
  if (!configure && !previous) refuse("persisted restart policy absent");
  const catalogues = {};
  for (const provider of ids) catalogues[provider] = await freshCatalogue(ctx.api, provider, selectedModel(selections, provider), ids);
  const policy = previous ?? { selections: { ...selections }, roster: ids, catalogues: {}, launches: [], identities: {}, first_sends: {} };
  Object.assign(policy.catalogues, catalogues);
  const config = await ctx.api("/api/config");
  const generalProvider = policy.general_provider ?? (roster.run.find((provider) => !provider.runtime)?.id ?? ids[0]);
  generalSelection(config, generalProvider, selections[generalProvider]);
  if (configure) {
    for (const provider of ids) await ctx.api("/api/config", { method: "PATCH", body: { key: `agents.${provider}.default_model`, value: selections[provider] } });
    await ctx.api("/api/config", { method: "PATCH", body: { key: "general_agent.model", value: selections[generalProvider] } });
  }
  const saved = await ctx.api("/api/config");
  generalSelection(saved, generalProvider, selections[generalProvider]);
  if (saved.general_agent?.model !== selections[generalProvider]) refuse("General model was not persisted");
  for (const provider of ids) if (saved.agents?.[provider]?.default_model !== selections[provider]) refuse(`${provider} scoped default was not persisted`);
  policy.general_provider = generalProvider;
  policy.configurations ??= {};
  policy.configurations[ctx.options.mode] = { general_provider: generalProvider, general_model: saved.general_agent.model,
    defaults: Object.fromEntries(ids.map((provider) => [provider, saved.agents[provider].default_model])) };
  ctx.state.model_policy = policy;
  ctx.save();
  const assertConfiguration = async (provider) => {
    const current = await ctx.api("/api/config");
    generalSelection(current, generalProvider, selections[generalProvider]);
    if (current.general_agent?.model !== selections[generalProvider]
      || current.agents?.[provider]?.default_model !== selections[provider]) refuse("scoped model configuration changed");
  };
  const guard = (body) => async (...args) => {
    if (policy.refusal) refuse(`persisted refusal: ${policy.refusal}`);
    try { return await body(...args); } catch (error) {
      policy.refusal = error.message;
      ctx.save();
      throw error;
    }
  };
  const boundary = {
    roster,
    async launch(provider, path, call) {
      const model = selectedModel(selections, provider);
      const catalogue = await freshCatalogue(ctx.api, provider, model, ids);
      await assertConfiguration(provider);
      policy.catalogues[provider] = catalogue;
      policy.launches.push({ provider, model, path, catalogue, checked_at: new Date().toISOString() });
      ctx.save();
      if (policy.refusal) refuse(`persisted refusal: ${policy.refusal}`);
      return call(model);
    },
    async identity(id, { first = false, restored = false } = {}) {
      let record = await readRecord(id);
      const provider = record?.agent_type;
      const selected = selectedModel(selections, provider);
      policy.catalogues[provider] = await freshCatalogue(ctx.api, provider, selected, ids);
      await assertConfiguration(provider);
      record = await readRecord(id);
      if (record?.id !== id) refuse(`tab ${id} own identity unavailable`);
      const previousIdentity = policy.identities[id];
      if (restored && !previousIdentity) refuse(`tab ${id} persisted identity absent`);
      const evidence = modelIdentity(record, provider, selected, { first, previous: restored ? previousIdentity : null });
      policy.identities[id] = evidence;
      if (restored) { policy.restored_identities ??= {}; policy.restored_identities[id] = evidence; }
      ctx.save();
      return evidence;
    },
    async send(id, call) {
      const fixture = ctx.state.tabs.find((tab) => tab.id === id);
      const first = fixture?.journey === "J1" && !policy.first_sends[id] && ctx.options.mode === "setup";
      const evidence = await boundary.identity(id, { first, restored: ctx.options.mode === "verify" });
      policy.first_sends[id] = true;
      ctx.save();
      if (policy.refusal) refuse(`persisted refusal: ${policy.refusal}`);
      return call(evidence);
    },
  };
  for (const name of ["launch", "identity", "send"]) boundary[name] = guard(boundary[name]);
  return boundary;
}
