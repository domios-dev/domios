import { Events } from "./env.mjs";
import { installed, selectProviders } from "./oracle.mjs";
import { fixtureProviderId } from "./fixture-control.mjs";
import { ModelPolicyRefusal, profileBacked, profileReady, profileSelection, profileIdentity, selectionDefault } from "./profile-policy.mjs";
import { JourneyFailure, observe, touchesTab } from "./wait.mjs";

// Bounds failure only: product readiness means usable, and the session's own evidence reports the model after it.
export const RESTORED_MODEL_DEADLINE_MS = 30_000;

const refuse = (reason) => { throw new ModelPolicyRefusal(reason); };
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

function verifyCacheQualification(descriptor, selection, refresh, context) {
  if (!context?.qualification) return false;
  const q = context.qualification, setup = context.setup_live;
  if (context.phase !== "verify" || !nonempty(context.run) || !nonempty(context.build_sha)
    || context.setup_build_sha !== context.build_sha || setup?.run !== context.run || setup.build_sha !== context.build_sha
    || q.run !== context.run || q.build_sha !== context.build_sha || q.model_id !== selection
    || q.verified_live_in_setup !== true || q.post_restart_refresh_in_flight !== true || q.model_present_in_cache !== true
    || setup.catalogue?.provider !== descriptor.id || setup.catalogue.selected !== selection
    || setup.catalogue.refresh?.refreshed !== true || setup.catalogue.refresh.discovery_required !== true
    || refresh?.refreshed !== false) refuse(`${descriptor.id} verify cache lacks same-run live setup evidence on the same build and model`);
  const entry = setup.catalogue;
  catalogueSelection({ id: entry.provider, availability: "available", model_catalogue: entry.capability,
    available_models: [entry.model], catalogue_version: entry.catalogue_version }, selection, entry.refresh);
  return true;
}

export function catalogueSelection(descriptor, selection, refresh, context) {
  if (!installed(descriptor)) refuse(`${descriptor?.id ?? "unknown"} unavailable`);
  const capability = descriptor.model_catalogue;
  if (!capability || !["authoritative", "free_form"].includes(capability.authority)
    || typeof capability.liveDiscoveryRequired !== "boolean") refuse(`${descriptor.id} catalogue capability unknown`);
  const verifiedCache = verifyCacheQualification(descriptor, selection, refresh, context);
  if (refresh?.provider_id !== descriptor.id || typeof refresh.refreshed !== "boolean"
    || typeof refresh.discovery_required !== "boolean" || refresh.discovery_required !== capability.liveDiscoveryRequired
    || (refresh.reason && !((verifiedCache || capability.liveDiscoveryRequired === false) && refresh.refreshed === false
      && refresh.reason === "the provider reported no live model list")) || (capability.liveDiscoveryRequired && !refresh.refreshed && !verifiedCache)) refuse(`${descriptor.id} catalogue refresh unavailable`);
  if (!Number.isSafeInteger(descriptor.catalogue_version) || !Number.isSafeInteger(refresh.version)) refuse(`${descriptor.id} catalogue version unknown`);
  if (!Array.isArray(descriptor.available_models) || !descriptor.available_models.length) refuse(`${descriptor.id} catalogue absent`);
  const matches = descriptor.available_models.filter((model) => model.id === selection);
  if (matches.length !== 1) refuse(`${descriptor.id} unknown or ambiguous advertised model ${selection}`);
  const model = matches[0];
  if (model.available === false || (model.availability != null && model.availability !== "available")) refuse(`${descriptor.id}/${selection} capability unavailable or unknown`);
  return { provider: descriptor.id, selected: selection, catalogue_version: descriptor.catalogue_version,
    capability, model, refresh, ...(verifiedCache ? { qualification: context.qualification } : {}) };
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

// The one state worth waiting on: every reported identity already agrees and only the actual model is absent.
export function restoredModelPending(record, provider, selected, catalogue, previous) {
  if (record?.agent_type !== provider || record.agent_session_model != null || record.model_fallback || record.model_identity_mismatch
    || !previous?.session || record.agent_session_id !== previous.session) return false;
  const profile = catalogue.kind === "profile";
  if (profile && record.agent_runtime_config?.launchProfile !== catalogue.launch_profile) return false;
  if (profile && catalogue.model == null) return previous.actual != null;
  const expected = profile ? catalogue.model : selected;
  return record.agent_requested_model === expected && record.agent_provider_resolved_model === expected;
}

async function confirmedRestoredRecord(ctx, id, record, readRecord, pending) {
  if (!pending(record)) return record;
  const signal = AbortSignal.timeout(RESTORED_MODEL_DEADLINE_MS);
  const open = ctx.modelEvents ?? ((deadline) => new Events(ctx.dev, deadline).open());
  let events;
  try {
    events = await open(signal);
    return await observe({
      events, filter: touchesTab(id), label: `tab ${id} actual model`,
      read: () => readRecord(id, signal),
      judge: (current) => pending(current) ? { pending: `tab ${id} actual model` } : { done: current },
    });
  } catch (error) {
    if (error instanceof JourneyFailure || signal.aborted) refuse(`tab ${id} actual model unconfirmed after ${RESTORED_MODEL_DEADLINE_MS} ms; no further paid turn permitted`);
    throw error;
  } finally {
    events?.close();
  }
}

export function generalSelection(config, provider, selected) {
  const general = config.general_agent;
  if (general?.model && general.model !== selected) refuse("conflicting General model override");
  if (general?.first_run_complete && general.agent_provider !== provider) refuse("conflicting General provider");
}

export async function refreshAfterStartup(ctx, provider) {
  const deadline = ctx.catalogueDeadline ?? (ms => AbortSignal.timeout(ms));
  const initialSignal = deadline(45_000), observation = new AbortController();
  const openingAbort = () => observation.abort();
  initialSignal.addEventListener("abort", openingAbort, { once: true });
  if (initialSignal.aborted) observation.abort();
  const open = ctx.catalogueEvents ?? ((signal) => new Events(ctx.dev, signal).open());
  const refresh = signal => ctx.api("/api/agents/providers/refresh", { method: "POST", body: { provider_id: provider }, signal });
  let events, initial, eventSignal, requestSignal = initialSignal, stage = "refresh";
  try {
    events = await open(observation.signal);
    initial = await refresh(requestSignal);
    initialSignal.removeEventListener("abort", openingAbort);
    if (initial?.discovery_required !== true || initial.refreshed !== false) return initial;
    stage = "event";
    eventSignal = deadline(15_000);
    for (;;) {
      const event = await events.next(eventSignal);
      if (event?.event_type === "model-catalogue-changed" && event.data?.provider === provider
        && Number.isSafeInteger(event.data.version) && event.data.version > initial.version) break;
    }
    stage = "refresh";
    requestSignal = deadline(45_000);
    return await refresh(requestSignal);
  } catch (error) {
    if ((stage === "event" ? eventSignal.aborted : requestSignal.aborted) || error?.message === "deadline") {
      if (stage === "event" && ctx.options?.mode === "verify" && initial?.discovery_required === true && initial.refreshed === false) {
        return { ...initial, startup_event_deadline: true };
      }
      refuse(stage === "event" ? `${provider} startup catalogue completion event did not arrive before the 15 s deadline`
        : `${provider} catalogue refresh response did not arrive before the 45 s deadline`);
    }
    throw error;
  } finally {
    initialSignal.removeEventListener("abort", openingAbort);
    observation.abort();
    events?.close();
  }
}

export async function freshCatalogue(api, provider, selected, expectedRoster, fixture = () => undefined, observedRefresh, context) {
  const read = async () => {
    const descriptors = (await api("/api/agents/providers")).filter(descriptor => descriptor.id !== fixture());
    if (expectedRoster && !same(descriptors.filter(installed).map(descriptor => descriptor.id), expectedRoster)) refuse("installed provider roster changed");
    return descriptors;
  };
  const descriptors = await read();
  const descriptor = descriptors.find(descriptor => descriptor.id === provider);
  if (profileBacked(descriptor)) return profileSelection(descriptor, selected, await api(`/api/agents/providers/${encodeURIComponent(provider)}/profiles`));
  const refresh = observedRefresh ?? await api("/api/agents/providers/refresh", { method: "POST", body: { provider_id: provider } });
  return catalogueSelection((observedRefresh ? descriptors : await read()).find(descriptor => descriptor.id === provider), selected, refresh, context);
}

export async function preparePolicy(ctx, { readRecord, configure = false, required } = {}) {
  const selections = Object.freeze({ ...ctx.options.models });
  const fixture = () => {
    const id = ctx.state.j3?.fixture_provider;
    if (id && id !== fixtureProviderId(ctx.state.run)) refuse("control fixture identity differs from this run");
    return id;
  };
  const descriptors = (await ctx.api("/api/agents/providers")).filter(descriptor => descriptor.id !== fixture());
  const roster = selectProviders(descriptors, { required });
  if (roster.missing.length) refuse(`required providers missing: ${roster.missing.map((entry) => entry.id).join(",")}`);
  const ids = roster.run.map((provider) => provider.id);
  if (ctx.options.providers && !same(ctx.options.providers, ids)) refuse("provider roster narrowing is forbidden");
  if (!same(Object.keys(selections), ids)) {
    const hints = [];
    for (const descriptor of descriptors.filter(descriptor => profileBacked(descriptor) && !Object.hasOwn(selections, descriptor.id))) {
      const profiles = await ctx.api(`/api/agents/providers/${encodeURIComponent(descriptor.id)}/profiles`);
      hints.push(`--model ${descriptor.id}=<profile> (ready profiles: ${profiles.filter(profileReady).map(profile => profile.id).join(", ") || "none"})`);
    }
    refuse(`explicit selections must cover the complete installed provider roster${hints.length ? `; expected ${hints.join("; ")}` : ""}`);
  }
  const previous = ctx.state.model_policy;
  if (previous?.refusal) refuse(`persisted refusal: ${previous.refusal}`);
  if (previous && (!same(previous.roster, ids) || !same(Object.entries(previous.selections).map((entry) => JSON.stringify(entry)), Object.entries(selections).map((entry) => JSON.stringify(entry))))) refuse("persisted restart policy differs from explicit selections or roster");
  if (!configure && !previous) refuse("persisted restart policy absent");
  const catalogues = {}, build = ctx.health?.build_sha ?? ctx.health?.build;
  for (const provider of ids) {
    if (profileBacked(descriptors.find(descriptor => descriptor.id === provider))) {
      catalogues[provider] = await freshCatalogue(ctx.api, provider, selectedModel(selections, provider), ids, fixture);
      continue;
    }
    const { startup_event_deadline, ...refresh } = await refreshAfterStartup(ctx, provider);
    const selected = selectedModel(selections, provider);
    const context = startup_event_deadline ? {
      phase: ctx.options.mode, run: ctx.state.run, build_sha: build,
      setup_build_sha: previous?.phase_builds?.setup, setup_live: previous?.setup_live?.[provider],
      qualification: { verified_live_in_setup: true, post_restart_refresh_in_flight: true,
        model_present_in_cache: true, model_id: selected, run: ctx.state.run, build_sha: build },
    } : undefined;
    catalogues[provider] = await freshCatalogue(ctx.api, provider, selected, ids, fixture, refresh, context);
  }
  const policy = previous ?? { selections: { ...selections }, roster: ids, catalogues: {}, launches: [], identities: {}, first_sends: {} };
  Object.assign(policy.catalogues, catalogues);
  policy.phase_builds ??= {};
  policy.phase_builds[ctx.options.mode] = build;
  if (ctx.options.mode === "setup") policy.setup_live = Object.fromEntries(Object.entries(catalogues)
    .filter(([, entry]) => entry.refresh?.refreshed === true && entry.refresh.discovery_required === true)
    .map(([provider, catalogue]) => [provider, { run: ctx.state.run, build_sha: build, catalogue }]));
  if (ctx.options.mode === "verify") policy.verify_catalogues = structuredClone(catalogues);
  const config = await ctx.api("/api/config");
  const generalProvider = policy.general_provider ?? (roster.run.find((provider) => !provider.runtime)?.id ?? ids[0]);
  if (catalogues[generalProvider].kind === "profile") refuse("General requires a provider with a flat advertised model selection");
  generalSelection(config, generalProvider, selections[generalProvider]);
  if (configure) {
    // A fresh instance has no per-provider entry, and the config PATCH only creates the last path segment.
    for (const provider of ids) await ctx.api("/api/config", { method: "PATCH", body: { key: `agents.${provider}`, value: { ...(config.agents?.[provider] ?? {}), default_model: selectionDefault(catalogues[provider]) } } });
    await ctx.api("/api/config", { method: "PATCH", body: { key: "general_agent.model", value: selections[generalProvider] } });
  }
  const saved = await ctx.api("/api/config");
  generalSelection(saved, generalProvider, selections[generalProvider]);
  if (saved.general_agent?.model !== selections[generalProvider]) refuse("General model was not persisted");
  for (const provider of ids) if ((saved.agents?.[provider]?.default_model ?? null) !== selectionDefault(catalogues[provider])) refuse(`${provider} scoped default was not persisted`);
  policy.general_provider = generalProvider;
  policy.configurations ??= {};
  policy.configurations[ctx.options.mode] = { general_provider: generalProvider, general_model: saved.general_agent.model,
    defaults: Object.fromEntries(ids.map((provider) => [provider, saved.agents[provider]?.default_model ?? null])) };
  ctx.state.model_policy = policy;
  ctx.save();
  const assertConfiguration = async (provider) => {
    const current = await ctx.api("/api/config");
    generalSelection(current, generalProvider, selections[generalProvider]);
    if (current.general_agent?.model !== selections[generalProvider]
      || (current.agents?.[provider]?.default_model ?? null) !== selectionDefault(catalogues[provider])) refuse("scoped model configuration changed");
  };
  const guard = (body) => async (...args) => {
    if (policy.refusal) refuse(`persisted refusal: ${policy.refusal}`);
    try { return await body(...args); } catch (error) {
      if (error instanceof ModelPolicyRefusal && !policy.refusal) {
        policy.refusal = error.message;
        ctx.save();
      }
      throw error;
    }
  };
  const boundary = {
    roster,
    async launch(provider, path, call) {
      const model = selectedModel(selections, provider);
      const catalogue = await freshCatalogue(ctx.api, provider, model, ids, fixture);
      await assertConfiguration(provider);
      policy.catalogues[provider] = catalogue;
      const profile = catalogue.kind === "profile";
      if (profile && ["J2-api-pair", "J2-ui-pair"].includes(path)) refuse("Team pair creation does not support launch profiles; choose a non-profile representative without narrowing J1");
      policy.launches.push({ provider, model: profile ? catalogue.model : model,
        ...(profile ? { selection: model, launch_profile: catalogue.launch_profile } : {}), path, catalogue, checked_at: new Date().toISOString() });
      ctx.save();
      if (policy.refusal) refuse(`persisted refusal: ${policy.refusal}`);
      return call(profile ? catalogue.model : model, profile ? catalogue.launch_profile : undefined);
    },
    async identity(id, { first = false, restored = false } = {}) {
      let record = await readRecord(id);
      const provider = record?.agent_type;
      const selected = selectedModel(selections, provider);
      policy.catalogues[provider] = await freshCatalogue(ctx.api, provider, selected, ids, fixture);
      await assertConfiguration(provider);
      record = await readRecord(id);
      if (record?.id !== id) refuse(`tab ${id} own identity unavailable`);
      const previousIdentity = policy.identities[id];
      if (restored && !previousIdentity) refuse(`tab ${id} persisted identity absent`);
      const options = { first, previous: restored ? previousIdentity : null };
      const catalogue = policy.catalogues[provider];
      if (restored && !first) {
        record = await confirmedRestoredRecord(ctx, id, record, readRecord,
          (current) => restoredModelPending(current, provider, selected, catalogue, previousIdentity));
        if (record?.id !== id) refuse(`tab ${id} own identity unavailable`);
      }
      const evidence = catalogue.kind === "profile" ? profileIdentity(record, provider, catalogue, options) : modelIdentity(record, provider, selected, options);
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
