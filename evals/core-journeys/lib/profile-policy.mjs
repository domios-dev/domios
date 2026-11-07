export class ModelPolicyRefusal extends Error {
  constructor(reason) { super(`model policy refused: ${reason}`); }
}
const refuse = reason => { throw new ModelPolicyRefusal(reason); };
const nonempty = value => typeof value === "string" && value.length > 0 && value.trim() === value;

export const profileBacked = descriptor => descriptor?.agent_runtime?.launchProfiles != null;
export const profileReady = profile => ["installed", "detected"].includes(profile?.install)
  && !["accepted", "installing", "failed"].includes(profile.setup?.state)
  && profile.auth !== "needs_auth";

export function profileSelection(descriptor, selection, profiles) {
  if (!profileBacked(descriptor) || !Array.isArray(profiles)) refuse(`${descriptor?.id} profile catalogue unavailable`);
  const slash = selection.indexOf("/");
  const id = slash < 0 ? selection : selection.slice(0, slash);
  const requested = slash < 0 ? null : selection.slice(slash + 1);
  const matches = profiles.filter(profile => profile.id === id);
  if (!Object.hasOwn(descriptor.agent_runtime.launchProfiles, id) || matches.length !== 1) refuse(`${descriptor.id} unknown or ambiguous launch profile ${id}`);
  const profile = matches[0];
  if (!profileReady(profile)) refuse(`${descriptor.id}/${id} launch profile is not installed and ready`);
  const options = (profile.configOptions ?? []).filter(option => option.category === "model" && option.options?.length);
  let model = requested;
  if (requested === "default" && !options.length) model = null;
  if (model != null && (!nonempty(model) || options.length !== 1
    || options[0].options.filter(option => option.value === model).length !== 1)) refuse(`${descriptor.id}/${id} unknown or ambiguous profile model ${requested}`);
  return { kind: "profile", provider: descriptor.id, selected: selection, launch_profile: id, model,
    profile: structuredClone(profile) };
}

export function profileIdentity(record, provider, catalogue, { first = false, previous = null } = {}) {
  if (record?.agent_type !== provider || record.agent_runtime_config?.launchProfile !== catalogue.launch_profile) refuse(`tab ${record?.id} launch profile mismatched or unknown`);
  if (record.model_fallback || record.model_identity_mismatch) refuse(`tab ${record.id} substitution reported`);
  const requested = record.agent_requested_model ?? null, resolved = record.agent_provider_resolved_model ?? null;
  const actual = record.agent_session_model ?? null;
  if (catalogue.model != null) {
    if (requested !== catalogue.model || resolved !== catalogue.model || (actual != null && actual !== catalogue.model)
      || (!first && actual !== catalogue.model)) refuse(`tab ${record.id} explicit profile model unknown or mismatched`);
  } else if (requested != null) refuse(`tab ${record.id} profile default has an unexpected requested model`);
  if (!first && !nonempty(record.agent_session_id)) refuse(`tab ${record.id} own session unknown`);
  if (previous && (!previous.session || previous.session !== record.agent_session_id
    || previous.launch_profile !== catalogue.launch_profile
    || (previous.actual != null && previous.actual !== actual))) refuse(`tab ${record.id} profile session or observed model changed`);
  return { provider, selected: catalogue.selected, launch_profile: catalogue.launch_profile,
    requested, resolved, actual, session: record.agent_session_id ?? null };
}

export function launchFlags(model, profile) {
  return [...(profile ? ["--profile", profile] : []), ...(model != null ? ["--model", model] : [])];
}

export function selectionDefault(entry) {
  return entry.kind === "profile" ? entry.model : entry.selected;
}
