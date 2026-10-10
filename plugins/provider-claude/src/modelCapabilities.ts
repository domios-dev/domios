import data from "../model-capabilities.json";
import type {ModelInfo} from "@codeterm/plugin-sdk";

interface Profile { efforts: Record<string,string>; defaultEffort?: string; contextWindowTokens: number }
export const MODEL_CAPABILITIES_FLAG = "--codeterm-model-capabilities";
export function modelCapabilities(model: string | null | undefined): Profile | null {
  const key = (data.models as Record<string,string>)[model || ""];
  return key ? (data.profiles as Record<string,Profile>)[key] || null : null;
}
export function modelWithCapabilities(model: ModelInfo): ModelInfo {
  const profile = modelCapabilities(model.id);
  if (!profile) return model;
  return {...model, reasoningEfforts:Object.keys(profile.efforts).map(id => ({id,displayName:id})),
    ...(profile.defaultEffort ? {defaultReasoningEffort:profile.defaultEffort} : {})};
}
export function reasoningEffortLaunchArgsForModel(model: string | null, effort: string): string[] {
  const mapped = modelCapabilities(model)?.efforts[effort];
  return mapped ? ["--effort",mapped] : model ? [] : effort ? ["--effort",effort] : [];
}
export function modelSettings(args: string[]): {args:string[]; env:Record<string,string>} {
  const enabled = args.includes(MODEL_CAPABILITIES_FLAG);
  const rest = args.filter(arg => arg !== MODEL_CAPABILITIES_FLAG);
  let model: string | undefined;
  for (let i=0; i<rest.length; i++) {
    if (rest[i] === "--model") model = rest[++i];
    else if (rest[i].startsWith("--model=")) model = rest[i].slice(8);
  }
  const profile = modelCapabilities(model);
  return {args:rest,env:enabled && profile ? {CLAUDE_CODE_AUTO_COMPACT_WINDOW:String(profile.contextWindowTokens)} : {}};
}
