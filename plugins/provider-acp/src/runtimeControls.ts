import type {JsonValue, RuntimeSettings, RuntimeAuthentication} from "@codeterm/plugin-sdk";
import {authenticationState, type AuthMethod} from "./auth";
const object=(value:unknown):Record<string,JsonValue>=>value !== null && typeof value === "object" && !Array.isArray(value) ? value as Record<string,JsonValue> : {};
export function label(value:unknown):value is string {
  if (typeof value !== "string" || !value.length || value.length > 256 || /[\u0000-\u001f\u007f-\u009f]/.test(value)) return false;
  let bytes=0;
  for (const character of value) {
    const point=character.codePointAt(0)!;
    if (point >= 0xd800 && point <= 0xdfff) return false;
    bytes+=point <= 0x7f ? 1 : point <= 0x7ff ? 2 : point <= 0xffff ? 3 : 4;
  }
  return bytes <= 256;
}
function description(value:unknown):string | undefined {
  if (typeof value !== "string") return undefined;
  const normalized=value.replace(/[\u0000-\u001f\u007f-\u009f\u2028\u2029]+/g," ").replace(/\s+/g," ").trim();
  if (!normalized.length) return undefined;
  let out="", bytes=0;
  for (const character of normalized) {
    const point=character.codePointAt(0)!;
    if (point >= 0xd800 && point <= 0xdfff) continue;
    const size=point <= 0x7f ? 1 : point <= 0x7ff ? 2 : point <= 0xffff ? 3 : 4;
    if (bytes + size > 512) break;
    out+=character;bytes+=size;
  }
  return out.length ? out : undefined;
}
function list(value:unknown, limit:number):Record<string,JsonValue>[] {
  return Array.isArray(value) && value.length <= limit ? value.map(object) : [];
}
export function commands(value:unknown):NonNullable<RuntimeSettings["commands"]> {
  const seen=new Set<string>();
  return list(value,128).flatMap(command=>{
    if (!label(command.name) || /[\s/]/.test(command.name) || seen.has(command.name)) return [];
    seen.add(command.name);
    const hint=object(command.input).hint;
    return [{name:command.name,description:typeof command.description === "string" ? command.description.slice(0,4096) : "",
      ...(typeof hint === "string" ? {argumentHint:hint.slice(0,4096)} : {})}];
  });
}
export function configOptions(value:unknown):NonNullable<RuntimeSettings["configOptions"]> {
  const seen=new Set<string>();
  return list(value,64).flatMap(config=>{
    if (!label(config.id) || !label(config.name) || seen.has(config.id)) return [];
    seen.add(config.id);
    const optionDescription=description(config.description);
    if (config.type === "boolean" && typeof config.currentValue === "boolean") {
      return [{id:config.id,name:config.name,kind:"boolean" as const,currentValue:config.currentValue ? "true" : "false",options:[],
        ...(label(config.category) ? {category:config.category} : {}),
        ...(optionDescription ? {description:optionDescription} : {})}];
    }
    const grouped:Array<{entry:Record<string,JsonValue>; group?:string}>=[];
    for (const entry of list(config.options,128)) {
      if (Array.isArray(entry.options)) {
        const groupLabel=label(entry.name) ? entry.name : label(entry.group) ? entry.group : undefined;
        for (const nested of list(entry.options,128)) grouped.push({entry:nested,group:groupLabel});
      } else {
        grouped.push({entry});
      }
    }
    const ids=new Set<string>();
    const options=grouped.slice(0,128).flatMap(({entry,group})=>{
      if (!label(entry.value) || !label(entry.name) || ids.has(entry.value)) return [];
      ids.add(entry.value);
      const choiceDescription=description(entry.description);
      return [{value:entry.value,name:entry.name,
        ...(choiceDescription ? {description:choiceDescription} : {}),
        ...(group ? {group} : {})}];
    });
    if (config.type !== "select" || !label(config.currentValue)) return [{id:config.id,name:config.name,currentValue:String(config.currentValue).slice(0,256) || "Unknown",options:[],
      ...(optionDescription ? {description:optionDescription} : {})}];
    return [{id:config.id,name:config.name,currentValue:config.currentValue,options,
      ...(label(config.category) ? {category:config.category} : {}),
      ...(optionDescription ? {description:optionDescription} : {})}];
  });
}
export function modelOption(settings:RuntimeSettings | undefined) {
  return settings?.configOptions?.find(option=>option.category === "model");
}
export function effortOption(settings:RuntimeSettings | undefined) {
  return settings?.configOptions?.find(option=>option.category === "thought_level");
}
export function sessionModel(value:unknown, declared:NonNullable<RuntimeSettings["configOptions"]>=[]) {
  if (modelOption({configOptions:declared})) return undefined;
  const models=object(value), seen=new Set<string>();
  const options=list(models.availableModels,128).flatMap(model=>{
    if (!label(model.modelId) || !label(model.name) || seen.has(model.modelId)) return [];
    seen.add(model.modelId);return [{value:model.modelId,name:model.name}];
  });
  if (!label(models.currentModelId) || !seen.has(models.currentModelId)) return undefined;
  let id="acp-session-model";
  while (declared.some(option=>option.id === id)) id+="-";
  return {id,name:"Model",category:"model",currentValue:models.currentModelId,options};
}
export function selectedSessionModel(settings:RuntimeSettings | undefined, id:string | undefined, value:unknown):RuntimeSettings | undefined {
  const option=settings?.configOptions?.find(option=>option.id === id);
  if (!option || !label(value) || !option.options.some(choice=>choice.value === value)) return undefined;
  return {configOptions:settings!.configOptions!.map(option=>option.id === id ? {...option,currentValue:value} : option),model:value};
}
export function sessionSettings(value:unknown):RuntimeSettings {
  const response=object(value), modes=object(response.modes);
  const seen=new Set<string>();
  const config=Array.isArray(response.configOptions) ? configOptions(response.configOptions) : undefined;
  const fallback=sessionModel(response.models,config);
  const declared=fallback ? [...(config ?? []).slice(0,63),fallback] : config;
  const model=modelOption({configOptions:declared})?.currentValue;
  const effort=effortOption({configOptions:declared})?.currentValue;
  return {...(declared ? {configOptions:declared,...(model ? {model} : {}),...(effort ? {effort} : {})} : {}),
    ...(Array.isArray(modes.availableModes) ? {modes:list(modes.availableModes,64).flatMap(mode=>{
      if (!label(mode.id) || !label(mode.name) || seen.has(mode.id)) return [];
      seen.add(mode.id);return [{id:mode.id,name:mode.name}];
    })} : {}),
    ...(label(modes.currentModeId) && seen.has(modes.currentModeId) ? {mode:modes.currentModeId} : {})};
}
export function authentication(methods:AuthMethod[], authenticated?:boolean, required?:boolean):RuntimeAuthentication {
  return {state:authenticationState(methods,{authenticated,required}),methods:methods.map(method=>({id:method.id,name:method.name,available:method.type === "agent"}))};
}
